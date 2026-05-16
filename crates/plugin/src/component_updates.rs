use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    net_entity::NetEntity,
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
