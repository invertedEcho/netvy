use std::{collections::HashMap, time::Duration};

use bevy::{prelude::*, time::common_conditions::on_timer};
use serde::{Deserialize, Serialize};

use crate::{
    NetvyConfiguration, NetvyMode, Owner, PeerId,
    disconnect::{ClientDisconnected, DespawnNetEntities},
    net_entity::NetEntityId,
    network_messages::{
        AppNetworkMessageExt, FromClient, MessageDirection, NetworkMessageTarget, ToClients,
        ToServer,
    },
};

/// This plugins purpose is to check whether a client is still "alive", or the client entity should
/// be despawned alongside with all entities that belonged to this client.
pub struct AliveCheckPlugin;

impl Plugin for AliveCheckPlugin {
    fn build(&self, app: &mut App) {
        app.register_network_message::<CheckAlive>(MessageDirection::ClientToServer);

        app.init_resource::<AliveChecks>();

        app.add_systems(
            Update,
            client_send_check_alive_message.run_if(
                resource_equals(NetvyMode::Client).and_then(on_timer(Duration::from_secs(1))),
            ),
        );

        app.add_systems(
            FixedUpdate,
            (
                server_read_check_alive_messages,
                server_tick_alive_checks,
                server_check_alive_messages,
            ),
        );
    }
}

/// Keeps track of when was the last time a client/peer sent a AliveCheck message.
/// e.g. Duration is a delta time
#[derive(Resource, Default)]
struct AliveChecks(pub HashMap<PeerId, f32>);

#[derive(Message, Serialize, Deserialize)]
struct CheckAlive;

fn client_send_check_alive_message(
    mut alive_check_message_writer: MessageWriter<ToServer<CheckAlive>>,
) {
    alive_check_message_writer.write(ToServer(CheckAlive));
}

fn server_read_check_alive_messages(
    mut alive_check_message_reader: MessageReader<FromClient<CheckAlive>>,
    mut alive_checks: ResMut<AliveChecks>,
) {
    for message in alive_check_message_reader.read() {
        let peer_id = message.source_client;

        alive_checks.0.insert(peer_id, 0.0);
    }
}

fn server_tick_alive_checks(mut alive_checks: ResMut<AliveChecks>, time: Res<Time>) {
    for delta in alive_checks.0.values_mut() {
        *delta += time.delta_secs();
    }
}

fn server_check_alive_messages(
    mut commands: Commands,
    mut alive_checks: ResMut<AliveChecks>,
    client_query: Query<(Entity, &PeerId), With<PeerId>>,
    net_entities: Query<(Entity, &Owner, &NetEntityId)>,
    netvy_configuration: Res<NetvyConfiguration>,
    mut message_writer: MessageWriter<ToClients<DespawnNetEntities>>,
    mut client_disconnect_message_writer: MessageWriter<ToClients<ClientDisconnected>>,
) {
    alive_checks.0.retain(|peer_id, last_alive_check| {
        let client_timed_out = *last_alive_check >= netvy_configuration.timeout_client_seconds;

        if client_timed_out {
            let message = ClientDisconnected {
                client: *peer_id,
            };

            client_disconnect_message_writer.write(ToClients {
                message,
                // Also send this message to the disconnected client wait that makes no sense it doesnt
                // receive the message if its disconnected? oh it does, we actually need it so we know
                // when to close the client UDP socket.
                target: NetworkMessageTarget::All,
            });
            let Some(client_entity) = client_query
                .iter()
                .find(|(_, peer_id2)| peer_id.0 == peer_id2.0)
                .map(|res| res.0)
            else {
                error!(
                    "The client entity for a client that has timed out could not be found. The client in question may therefore not have been correctly despawned."
                );
                return false;
            };

            commands.entity(client_entity).despawn();
            info!(?client_entity, "Despawned timed out client entity");

            let mut net_entities_despawned: Vec<NetEntityId> = vec![];
            for (entity, owner, net_entity_id) in net_entities {
                if owner.0.0 == peer_id.0 {
                    debug!(?entity, "Despawning entity for timed out client");
                    commands.entity(entity).despawn();
                    net_entities_despawned.push(*net_entity_id);
                }
            }
            let message = DespawnNetEntities(net_entities_despawned);
            message_writer.write(ToClients { message, target: NetworkMessageTarget::Except(vec![*peer_id])});

            info!(
                ?client_entity,
                "Despawned all entities owned by a timed out client and notified all clients"
            );

            return false;
        }
        true
    });
}
