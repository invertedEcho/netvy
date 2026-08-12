use crate::{
    Authority, NetvyMode, OurPeerId, component_updates::component_registry::AppComponentExt,
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub struct SyncPositionPlugin;

impl Plugin for SyncPositionPlugin {
    fn build(&self, app: &mut App) {
        app.register_component::<InternalSyncPosition>();
        app.register_component::<SyncPosition>();

        app.add_systems(
            Update,
            (
                apply_internal_sync_position,
                add_internal_sync_position_component,
            ),
        );
    }
}

// Because vec3 doesnt implement bincode::encode and bincode::decode, we use three f32 instead
#[derive(Component, Serialize, Deserialize, Reflect, Debug)]
pub struct InternalSyncPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Add this component to entities of which position (transform.translation) you want to be synced across clients
#[derive(Component, Serialize, Deserialize, Debug)]
pub struct SyncPosition {
    /// Whether to linearly interpolate position updates on clients. Defaults to true
    linear_interpolation: bool,
}

impl Default for SyncPosition {
    fn default() -> Self {
        SyncPosition {
            linear_interpolation: true,
        }
    }
}

// TODO: this would break if the user wants to run physics on entities that he doesnt own
fn apply_internal_sync_position(
    mut commands: Commands,
    query: Query<(
        Entity,
        Option<&mut Transform>,
        &mut InternalSyncPosition,
        &Authority,
        &SyncPosition,
    )>,
    time: Res<Time>,
    our_peer_id: Option<Res<OurPeerId>>,
    netvy_mode: Res<NetvyMode>,
) {
    let Some(our_peer_id) = our_peer_id else {
        warn!(?netvy_mode, "Yeah OurPeerId doesnt exist");
        return;
    };
    for (entity, transform, mut internal_sync_position, authority, sync_position) in query {
        let x = internal_sync_position.x;
        let y = internal_sync_position.y;
        let z = internal_sync_position.z;

        let Some(mut transform) = transform else {
            commands.entity(entity).insert(Transform::from_xyz(x, y, z));
            continue;
        };

        if authority.0.0 == our_peer_id.0.0 {
            // debug!(
            //     ?authority,
            //     ?our_peer_id,
            //     "We have authority, applying transform to internal_sync_position. internal_sync_position = transform;"
            // );
            internal_sync_position.x = transform.translation.x;
            internal_sync_position.y = transform.translation.y;
            internal_sync_position.z = transform.translation.z;
        } else {
            // debug!(
            //     ?authority,
            //     ?our_peer_id,
            //     "We dont have authority, applying internal_sync_position to transform. transform = internal_sync_position;"
            // );
            if sync_position.linear_interpolation {
                let current = transform.translation;
                let target = vec3(x, y, z);
                let lerp_factor = (10.0 * time.delta_secs()).clamp(0.0, 1.0);

                let new_translation = current.lerp(target, lerp_factor);

                transform.translation = new_translation;
            } else {
                transform.translation = vec3(x, y, z);
            }
        }
    }
}

fn add_internal_sync_position_component(
    query: Query<(Entity, &Transform), Added<SyncPosition>>,
    mut commands: Commands,
) {
    for (entity, transform) in query {
        let position = transform.translation;
        commands.entity(entity).insert(InternalSyncPosition {
            x: position.x,
            y: position.y,
            z: position.z,
        });
    }
}
