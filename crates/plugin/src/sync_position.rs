use crate::NetEntityType;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// Because vec3 doesnt implement bincode::encode and bincode::decode, we use three f32 instead
#[derive(Component, Serialize, Deserialize, Reflect, Debug)]
pub struct InternalSyncPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Add this component to entities of which position (transform.translation) you want to be synced across clients
#[derive(Component, Serialize, Deserialize)]
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

// TODO: this would break if the user wants to run physics on entities with NetEntityType::Remote
pub fn apply_internal_sync_position(
    mut commands: Commands,
    query: Query<
        (
            Entity,
            Option<&mut Transform>,
            &mut InternalSyncPosition,
            &NetEntityType,
            &SyncPosition,
        ),
        Or<(Changed<Transform>, Changed<InternalSyncPosition>)>,
    >,
    time: Res<Time>,
) {
    for (entity, transform, mut internal_sync_position, entity_type, sync_position) in query {
        let x = internal_sync_position.x;
        let y = internal_sync_position.y;
        let z = internal_sync_position.z;

        if let Some(mut transform) = transform {
            match entity_type {
                NetEntityType::Local => {
                    internal_sync_position.x = transform.translation.x;
                    internal_sync_position.y = transform.translation.y;
                    internal_sync_position.z = transform.translation.z;
                }
                NetEntityType::Remote => {
                    if sync_position.linear_interpolation {
                        let current = transform.translation;
                        let target = vec3(x, y, z);
                        let lerp_factor = (10.0 * time.delta_secs()).clamp(0.0, 1.0);

                        transform.translation = current.lerp(target, lerp_factor);
                    } else {
                        transform.translation = vec3(x, y, z);
                    }
                }
            }
        } else {
            commands.entity(entity).insert(Transform::from_xyz(x, y, z));
        }
    }
}
