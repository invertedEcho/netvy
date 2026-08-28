use crate::{
    Authority, OurPeerId, SyncMode, component_updates::component_registry::AppComponentExt,
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub mod prelude {
    pub use crate::sync_position::{InternalSyncPosition, SyncPosition};
}

pub struct SyncPositionPlugin;

impl Plugin for SyncPositionPlugin {
    fn build(&self, app: &mut App) {
        app.register_component_with_sync_mode::<InternalSyncPosition>(SyncMode::FixedRate(0.05));
        app.register_component::<SyncPosition>();

        app.add_systems(
            Update,
            (apply_internal_sync_position, add_required_components),
        );
    }
}

// Because vec3 doesnt derive bincode::encode and bincode::decode, we create our own component
#[derive(Component, Serialize, Deserialize, Reflect, Debug, Default)]
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
    query: Query<(
        &mut Transform,
        &mut InternalSyncPosition,
        &Authority,
        &SyncPosition,
    )>,
    time: Res<Time>,
    our_peer_id: If<Res<OurPeerId>>,
) {
    for (mut transform, mut internal_sync_position, authority, sync_position) in query {
        let x = internal_sync_position.x;
        let y = internal_sync_position.y;
        let z = internal_sync_position.z;

        if authority.0.0 == our_peer_id.0.0.0 {
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

/// Ensures all required components are present on entities with SyncPosition component.
fn add_required_components(query: Query<Entity, Added<SyncPosition>>, mut commands: Commands) {
    for entity in query {
        info!(
            ?entity,
            "Adding required components to new SyncPosition entity"
        );
        commands
            .entity(entity)
            .insert((InternalSyncPosition::default(), Transform::default()));
    }
}
