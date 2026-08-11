use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    Owner, PeerId,
    net_entity::NetEntityId,
    network_messages::{
        AppNetworkMessageExt, FromClient, FromServer, MessageDirection, NetworkMessageTarget,
        ToClients, ToServer,
    },
};

pub struct DisconnectPlugin;

impl Plugin for DisconnectPlugin {
    fn build(&self, app: &mut App) {
        app.register_network_message::<DespawnNetEntities>(MessageDirection::ServerToClient);
        app.register_network_message::<InternalDisconnectMessage>(MessageDirection::ClientToServer);

        app.add_systems(
            FixedUpdate,
            (
                read_despawn_net_entities_messages,
                handle_internal_disconnect_message,
            ),
        );

        app.add_observer(handle_disconnect_event);
    }
}

#[derive(Event)]
pub struct Disconnect;

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

#[derive(Message, Serialize, Deserialize)]
struct InternalDisconnectMessage;

// This is really just a shorthand to trigger a disconnect, instead of having to use a messagewriter
fn handle_disconnect_event(
    on: On<Disconnect>,
    mut message_writer: MessageWriter<ToServer<InternalDisconnectMessage>>,
) {
    message_writer.write(ToServer(InternalDisconnectMessage));
}

fn handle_internal_disconnect_message(
    mut message_reader: MessageReader<FromClient<InternalDisconnectMessage>>,
    mut commands: Commands,
    client_query: Query<(Entity, &PeerId)>,
    net_entities: Query<(Entity, &Owner, &NetEntityId)>,
    mut message_writer: MessageWriter<ToClients<DespawnNetEntities>>,
) {
    for message in message_reader.read() {
        let peer_id_client = message.source_client;
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
                target: NetworkMessageTarget::All,
            });
        }
    }
}

#[derive(Message)]
struct ClientDisconnected {
    client: PeerId,
}
