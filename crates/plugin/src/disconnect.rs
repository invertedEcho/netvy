use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    net_entity::NetEntityId,
    network_messages::{AppNetworkMessageExt, FromServer, MessageDirection},
};

pub struct DisconnectPlugin;

impl Plugin for DisconnectPlugin {
    fn build(&self, app: &mut App) {
        app.register_network_message::<DespawnNetEntities>(MessageDirection::ServerToClient);

        app.add_systems(FixedUpdate, read_despawn_net_entities_messages);
    }
}

#[derive(Event)]
struct Disconnect;

#[derive(Message, Serialize, Deserialize)]
struct DespawnNetEntities(Vec<NetEntityId>);

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

            commands.entity(entity);
        }
    }
}

fn handle_disconnect_event(on: On<Disconnect>) {}
