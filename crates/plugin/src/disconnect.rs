use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    ClientSocket, NetvyMode, OurPeerId, Owner, PeerId,
    alive_check::AliveChecks,
    net_entity::NetEntityId,
    network_messages::{
        AppNetworkMessageExt, FromClient, FromServer, MessageDirection, NetworkMessageTarget,
        ToClients, ToServer,
    },
    server::{ConnectedClients, SocketAddrToPeerId},
    utils::reverse_hash_map_lookup,
};

pub mod prelude {
    pub use crate::disconnect::{ClientDisconnected, Disconnect};
}

pub struct DisconnectPlugin;

impl Plugin for DisconnectPlugin {
    fn build(&self, app: &mut App) {
        app.register_network_message::<DespawnNetEntities>(MessageDirection::ServerToClient);
        app.register_network_message::<ClientDisconnected>(MessageDirection::ServerToClients);
        app.register_network_message::<InternalDisconnectMessage>(MessageDirection::ClientToServer);

        app.add_systems(
            FixedUpdate,
            (
                read_despawn_net_entities_messages,
                handle_internal_disconnect_message.run_if(
                    resource_equals(NetvyMode::Server)
                        .or_else(resource_equals(NetvyMode::HostClient)),
                ),
                handle_client_disconnected_message,
            ),
        );

        app.add_observer(handle_disconnect_event);
    }
}

/// Trigger this event on the client to disconnect from the server. The client entity will be
/// despawned from all connected peers alongside all net entities belonging to that client.
#[derive(Event)]
pub struct Disconnect;

// internal network message, sent from server to clients to despawn all net entities of
// disconnected/timed out client.
#[derive(Message, Serialize, Deserialize)]
pub struct DespawnNetEntities(pub Vec<NetEntityId>);

fn read_despawn_net_entities_messages(
    mut commands: Commands,
    mut message_reader: MessageReader<FromServer<DespawnNetEntities>>,
    net_entities: Query<(Entity, &NetEntityId)>,
) {
    for message in message_reader.read() {
        for net_entity_to_despawn in &message.0.0 {
            let Some(entity) = net_entities
                .iter()
                .find(|(_, net_entity)| net_entity.0 == net_entity_to_despawn.0)
                .map(|(entity, _)| entity)
            else {
                error!(
                    ?net_entity_to_despawn,
                    "Failed to despawn entity from DespawnNetEntities message: No entity exists with the given NetEntityId"
                );
                continue;
            };

            commands.entity(entity).despawn();
        }
    }
}

/// A client can trigger the `Disconnect` event. netvy will handle this event and send this
/// `InternalDisconnectMessage` network message to the server.
#[derive(Message, Serialize, Deserialize)]
struct InternalDisconnectMessage;

fn handle_disconnect_event(
    _: On<Disconnect>,
    mut commands: Commands,
    client_query: Query<(Entity, &PeerId)>,
    mut message_writer: MessageWriter<ToServer<InternalDisconnectMessage>>,
    our_peer_id: Option<Res<OurPeerId>>,
) {
    let Some(our_peer_id) = our_peer_id else {
        info!("Disconnect was triggered but OurPeerId doesn't exist, ignoring.");
        return;
    };

    let Some((client_entity, _)) = client_query
        .iter()
        .find(|(_, peer_id)| peer_id.0 == our_peer_id.0.0)
    else {
        error!("Disconnect was triggered but couldnt find our own client entity!");
        return;
    };

    commands.entity(client_entity).despawn();

    message_writer.write(ToServer(InternalDisconnectMessage));
}

fn handle_internal_disconnect_message(
    mut message_reader: MessageReader<FromClient<InternalDisconnectMessage>>,
    mut commands: Commands,
    client_query: Query<(Entity, &PeerId)>,
    net_entities: Query<(Entity, &Owner, &NetEntityId)>,
    mut message_writer: MessageWriter<ToClients<DespawnNetEntities>>,
    mut client_disconnect_message_writer: MessageWriter<ToClients<ClientDisconnected>>,
    mut alive_checks: ResMut<AliveChecks>,
    mut connected_clients: ResMut<ConnectedClients>,
    socket_addr_to_peer_id: Res<SocketAddrToPeerId>,
) {
    for message in message_reader.read() {
        let peer_id_client = message.source_client;
        let socket_addr = reverse_hash_map_lookup(&socket_addr_to_peer_id.0, peer_id_client)
            .expect("Invariant violation: A PeerId must always have a SocketAddr.");
        let index = connected_clients.0.iter().position(|s| *s == socket_addr).expect("Invariant violation: A SocketAddr contained in SocketAddrToPeerId must also be contained in ConnectedClients.");

        connected_clients.0.swap_remove(index);

        alive_checks.0.remove(&peer_id_client);

        if let Some(client_entity) = client_query
            .iter()
            .find(|(_, peer_id)| peer_id.0 == peer_id_client.0)
            .map(|(entity, _)| entity)
        {
            commands.entity(client_entity).despawn();
        }
        let mut net_entities_to_despawn: Vec<NetEntityId> = vec![];
        for (entity, owner, net_entity_id) in net_entities {
            if owner.0.0 == peer_id_client.0 {
                commands.entity(entity).despawn();
                net_entities_to_despawn.push(*net_entity_id);
            }
        }
        if !net_entities_to_despawn.is_empty() {
            let message = DespawnNetEntities(net_entities_to_despawn);
            message_writer.write(ToClients {
                message,
                target: NetworkMessageTarget::Except(vec![peer_id_client]),
            });
        }

        let message = ClientDisconnected {
            client: peer_id_client,
        };

        client_disconnect_message_writer.write(ToClients {
            message,
            // Also send this message to the disconnected client wait that makes no sense it doesnt
            // receive the message if its disconnected? oh it does, we actually need it so we know
            // when to close the client UDP socket.
            // TODO: right now we dont actually close the UDP socket on the client. i think we are
            // missing a bunch of cleanup for disconnected clients.
            target: NetworkMessageTarget::All,
        });
    }
}

/// This message can be used to be informed whenever a client has disconnected. It is sent from the
/// server to all connected clients. Read it on the client.
#[derive(Message, Serialize, Deserialize)]
pub struct ClientDisconnected {
    pub client: PeerId,
}

fn handle_client_disconnected_message(
    mut message_reader: MessageReader<FromServer<ClientDisconnected>>,
    our_peer_id: Option<Res<OurPeerId>>,
    mut commands: Commands,
) {
    for message in message_reader.read() {
        let Some(ref our_peer_id) = our_peer_id else {
            error!(
                "Received ClientDisconnected message but OurPeerId doesnt exist, cant determine whether to close our UDP socket"
            );
            continue;
        };
        if message.0.client.0 == our_peer_id.0.0 {
            // is removing the resource enough? it should drop the udp socket, effectively closing it?
            commands.remove_resource::<ClientSocket>();
        }
    }
}
