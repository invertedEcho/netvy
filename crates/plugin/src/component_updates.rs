use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use bevy::prelude::*;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    BINCODE_CONFIG, CurrentSocket,
    datagram::build_component_update_datagram,
    get_or_create_mut_update_sequence_number,
    net_entity::{NetEntity, NetEntityType},
    registry::{ComponentRegistry, ComponentTypeId},
};

/// Stores the sequence number of component updates for each net entity and a corresponding component type id
/// Used to ensure only newer updates are applied as UDP is unordered
#[derive(Resource, Clone, Reflect, Default)]
pub struct UpdateSequence(pub HashMap<(NetEntity, ComponentTypeId), u32>);

/// Stores component updates that failed to apply locally, for example no entity exists yet with the
/// given `net_entity_id`
#[derive(Resource, Default)]
pub struct FailedApplyComponentUpdates(pub Vec<FailedApplyComponentUpdate>);

pub struct FailedApplyComponentUpdate {
    pub component_type_id: ComponentTypeId,
    // We store the NetEntityId and not the Entity itself in case the update failed because of a
    // missing local entity (not yet spawned)
    pub net_entity_id: NetEntity,
    pub component_bytes: Vec<u8>,
    pub incoming_update_sequence: u32,
}

/// Stores failed component updates that could not be sent to the server.
/// For example, an entity with a registered component changed locally, but that entity doesn't have a
/// NetEntityId yet.
#[derive(Resource, Default)]
pub struct FailedSentComponentUpdates(pub Vec<FailedSentComponentUpdate>);

pub struct FailedSentComponentUpdate {
    pub entity: Entity,
    pub component_bytes: Vec<u8>,
    pub component_type_id: u8,
}

pub fn handle_send_interval_timer(
    time: Res<Time>,
    mut component_registry: ResMut<ComponentRegistry>,
) {
    for timer in component_registry.timer.values_mut() {
        timer.tick(time.delta());
    }
}

pub fn send_component_updates_fixed_rate<C>(
    component_registry: Res<ComponentRegistry>,
    entities: Query<(Entity, &C, Option<&NetEntity>, &NetEntityType)>,
    mut update_sequence: ResMut<UpdateSequence>,
    current_socket: If<Res<CurrentSocket>>,
    mut failed_sent_component_updates: ResMut<FailedSentComponentUpdates>,
) where
    C: Component + Serialize + DeserializeOwned,
{
    for (entity, component, maybe_net_entity_id, net_entity_type) in entities {
        // only send changes of our own entities
        if *net_entity_type != NetEntityType::Local {
            continue;
        }

        let type_id = TypeId::of::<C>();

        let Some(component_type_id) = component_registry
            .type_id_to_component_type_id
            .get(&type_id)
        else {
            error!("Couldnt get component type id by type id");
            return;
        };

        // we have one timer per component type id / registered component with sync mode fixed rate
        let Some(timer) = component_registry.timer.get(component_type_id) else {
            error!("Couldnt get timer for {component_type_id:?}");
            return;
        };

        let component_bytes = bincode::serde::encode_to_vec(component, BINCODE_CONFIG).unwrap();

        if !timer.is_finished() {
            return;
        };

        let Some(net_entity_id) = maybe_net_entity_id else {
            info!(
                "Failed to get net entity id for entity {entity:?}, adding to FailedSentComponentUpdates"
            );
            failed_sent_component_updates
                .0
                .push(FailedSentComponentUpdate {
                    component_bytes,
                    component_type_id: *component_type_id,
                    entity,
                });
            return;
        };

        let current_update_sequence = get_or_create_mut_update_sequence_number(
            &mut update_sequence,
            *net_entity_id,
            *component_type_id,
        );

        *current_update_sequence += 1;

        let component_update_bytes = build_component_update_datagram(
            &component_bytes,
            *component_type_id,
            net_entity_id,
            *current_update_sequence,
        );

        // send data of changed entity / comp to server
        let _ = current_socket.0.0.send(&component_update_bytes);
    }
}

pub fn detect_registered_component_change<C>(
    component_registry: Res<ComponentRegistry>,
    changed_entities: Query<(Entity, &C, Option<&NetEntity>, Option<&NetEntityType>), Changed<C>>,
    current_socket: If<Res<CurrentSocket>>,
    mut failed_sent_component_updates: ResMut<FailedSentComponentUpdates>,
    mut update_sequence: ResMut<UpdateSequence>,
) where
    C: Component + Serialize + DeserializeOwned,
{
    for (entity, changed_component, maybe_net_entity_id, maybe_net_entity_type) in changed_entities
    {
        let component_bytes =
            bincode::serde::encode_to_vec(changed_component, BINCODE_CONFIG).unwrap();

        let type_id = changed_component.type_id();

        let component_type_id = component_registry
            .type_id_to_component_type_id
            .get(&type_id)
            .expect("Given Component must be registered");

        // its possible that NetEntityType isnt present
        // -> `add_entity_type_to_sync_entities` runs after the Changed<> detection on this system
        let Some(net_entity_type) = maybe_net_entity_type else {
            failed_sent_component_updates
                .0
                .push(FailedSentComponentUpdate {
                    entity,
                    component_bytes,
                    component_type_id: *component_type_id,
                });
            continue;
        };

        if *net_entity_type != NetEntityType::Local {
            continue;
        }

        let Some(net_entity_id) = maybe_net_entity_id else {
            failed_sent_component_updates
                .0
                .push(FailedSentComponentUpdate {
                    component_bytes,
                    component_type_id: *component_type_id,
                    entity,
                });
            continue;
        };

        let current_update_sequence = get_or_create_mut_update_sequence_number(
            &mut update_sequence,
            *net_entity_id,
            *component_type_id,
        );

        *current_update_sequence += 1;

        let component_update = build_component_update_datagram(
            &component_bytes,
            *component_type_id,
            net_entity_id,
            *current_update_sequence,
        );

        let _ = current_socket.0.0.send(&component_update);
    }
}
