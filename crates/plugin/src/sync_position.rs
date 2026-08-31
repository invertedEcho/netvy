use crate::{
    Authority, OurPeerId, SyncMode, component_updates::component_registry::AppComponentExt,
    net_entity::NetEntityId,
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub mod prelude {
    pub use crate::sync_position::{SyncPosition, TeleportNetEntity};
}

pub struct SyncPositionPlugin;

impl Plugin for SyncPositionPlugin {
    fn build(&self, app: &mut App) {
        app.register_component_with_sync_mode::<NetworkPosition>(SyncMode::FixedRate(0.05));
        app.register_component::<SyncPosition>();
        app.register_component::<ForceSyncPosition>();

        app.add_systems(
            Update,
            (apply_internal_sync_position, add_required_components),
        );
    }
}

// Because vec3 doesnt derive bincode::encode and bincode::decode, we create our own component
#[derive(Component, Serialize, Deserialize, Reflect, Debug, Default)]
pub struct NetworkPosition(pub Vec3);

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

#[derive(Component, Serialize, Deserialize)]
struct ForceSyncPosition(pub Vec3);

/// If you want to "teleport" a net entity on the server, while the client has authority, queue the `TeleportNetEntity` command.
/// If you want to frequently move a net entity on the server, you should instead give the server authority, e.g. by inserting the `Authority` component.
/// This will change the position on all connected peers.
///
/// Usage:
/// ```rust
/// commands.queue(TeleportNetEntity {
///     net_entity_id,
///     position
/// });
/// ```
pub struct TeleportNetEntity {
    pub net_entity_id: NetEntityId,
    pub position: Vec3,
}

impl Command for TeleportNetEntity {
    type Out = ();

    fn apply(self, world: &mut World) {
        let Some(entity) = world
            .query::<(Entity, &NetEntityId)>()
            .iter(world)
            .find_map(|(entity, net_entity_id)| {
                if net_entity_id.0 == self.net_entity_id.0 {
                    return Some(entity);
                } else {
                    return None;
                }
            })
        else {
            error!(net_entity_id = ?self.net_entity_id, "TeleportNetEntity command failed! The given NetEntityId could not be found.");
            return;
        };
        world
            .entity_mut(entity)
            .insert(ForceSyncPosition(self.position));
    }
}

// TODO: this would break if the user wants to run physics on entities that he doesnt own
// overall this system is a bit brittle. what if we want to change the transform on the server, but
// a client has authority? we also cant just add if cond for is server, as transform will change on
// server, literally changed by this system.
// what if we require user to insert temp component to ignore transform changes? and only sync?
// but thats stupid. i wonder how lightyear does this.
// i think lightyear works with avian instead. but the issue remains the same. which side does
// replicate when? what if both need to?...
fn apply_internal_sync_position(
    mut commands: Commands,
    query: Query<(
        Entity,
        &mut Transform,
        &mut NetworkPosition,
        &Authority,
        &SyncPosition,
        Option<&ForceSyncPosition>,
    )>,
    time: Res<Time>,
    our_peer_id: If<Res<OurPeerId>>,
) {
    for (
        entity,
        mut transform,
        mut internal_sync_position,
        authority,
        sync_position,
        force_sync_position,
    ) in query
    {
        // This is used by TeleportNetEntity command
        if let Some(force_sync_position) = force_sync_position {
            transform.translation = force_sync_position.0;
            commands.entity(entity).remove::<ForceSyncPosition>();
        } else {
            if authority.0.0 == our_peer_id.0.0.0 {
                // debug!(
                //     ?authority,
                //     ?our_peer_id,
                //     "We have authority, applying transform to internal_sync_position. internal_sync_position = transform;"
                // );
                internal_sync_position.0 = transform.translation;
            } else {
                // debug!(
                //     ?authority,
                //     ?our_peer_id,
                //     "We dont have authority, applying internal_sync_position to transform. transform = internal_sync_position;"
                // );
                if sync_position.linear_interpolation {
                    let current = transform.translation;
                    let lerp_factor = (10.0 * time.delta_secs()).clamp(0.0, 1.0);

                    let new_translation = current.lerp(internal_sync_position.0, lerp_factor);

                    transform.translation = new_translation;
                } else {
                    transform.translation = internal_sync_position.0;
                }
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
            .insert((NetworkPosition::default(), Transform::default()));
    }
}
