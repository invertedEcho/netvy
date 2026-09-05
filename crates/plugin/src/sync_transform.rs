use crate::{
    Authority, OurPeerId, SyncMode, component_updates::component_registry::AppComponentExt,
    net_entity::NetEntityId,
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub mod prelude {
    pub use crate::sync_transform::{
        AlternateSourceRotation, AlternateTargetRotation, SyncPosition, SyncRotation,
        TeleportNetEntity,
    };
}

pub struct SyncTransform;

impl Plugin for SyncTransform {
    fn build(&self, app: &mut App) {
        app.register_component_with_sync_mode::<NetworkPosition>(SyncMode::FixedRate(0.05));
        app.register_component::<SyncPosition>();
        app.register_component::<ForceSyncPosition>();

        app.register_component::<SyncRotation>();
        app.register_component_with_sync_mode::<NetworkRotation>(SyncMode::FixedRate(0.05));

        app.add_systems(
            Update,
            (
                apply_network_position,
                add_required_components_position,
                add_required_components_rotation,
                apply_network_rotation,
            ),
        );
    }
}

// We need an intermediate component to differentiate between networked position and bevys transform
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

#[derive(Component, Serialize, Deserialize, Reflect, Default)]
pub struct NetworkRotation(pub Quat);

/// Add this component to entities of which rotation (transform.rotation) you want to be synced across clients
#[derive(Component, Serialize, Deserialize)]
pub struct SyncRotation {
    /// Whether to spherical linear interpolate rotation updates on clients. Defaults to true
    pub linear_interpolation: bool,
    /// Whether to lock (not apply) roll rotation
    pub lock_roll: bool,
    /// Whether to lock (not apply) yaw rotation
    pub lock_yaw: bool,
    /// Whether to lock (not apply) pitch rotation
    pub lock_pitch: bool,
}

/// Insert this component to use the rotation from this entity instead for the rotation of the given rotation entity, instead of the rotation where the SyncRotation component is inserted
#[derive(Component)]
pub struct AlternateSourceRotation(pub NetEntityId);

/// Add this component to an entity which should be used as an alternate target on which to apply
/// the network rotation, from the specified net entity.
///
/// This is not replicated across peers.
#[derive(Component)]
pub struct AlternateTargetRotation(pub NetEntityId);

impl Default for SyncRotation {
    fn default() -> Self {
        SyncRotation {
            linear_interpolation: true,
            lock_roll: false,
            lock_yaw: false,
            lock_pitch: false,
        }
    }
}

impl SyncRotation {
    // TODO: might run into gimbal lock
    pub fn apply_rotation_locks(&self, src: &Quat) -> Quat {
        if !self.lock_roll && !self.lock_yaw && !self.lock_pitch {
            return *src;
        }

        let (yaw, pitch, roll) = src.to_euler(EulerRot::YXZ);

        Quat::from_euler(
            EulerRot::YXZ,
            if self.lock_yaw { 0.0 } else { yaw },
            if self.lock_pitch { 0.0 } else { pitch },
            if self.lock_roll { 0.0 } else { roll },
        )
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
                    Some(entity)
                } else {
                    None
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

fn apply_network_position(
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
        mut network_position,
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
                network_position.0 = transform.translation;
            } else {
                if sync_position.linear_interpolation {
                    let current = transform.translation;
                    let lerp_factor = (10.0 * time.delta_secs()).clamp(0.0, 1.0);

                    let new_translation = current.lerp(network_position.0, lerp_factor);

                    transform.translation = new_translation;
                } else {
                    transform.translation = network_position.0;
                }
            }
        }
    }
}

fn apply_network_rotation(
    query: Query<(
        Entity,
        &mut NetworkRotation,
        &Authority,
        &SyncRotation,
        &NetEntityId,
    )>,
    time: Res<Time>,
    our_peer_id: If<Res<OurPeerId>>,
    mut transform_query: Query<&mut Transform>,
    alternate_target_rotation: Query<(Entity, &AlternateTargetRotation)>,
    alternate_source_rotation: Query<(Entity, &AlternateSourceRotation)>,
) {
    for (entity, mut network_rotation, authority, sync_rotation, net_entity_id) in query {
        let our_entity = authority.0 == our_peer_id.0.0;

        if our_entity {
            // which entity to use for the source of the rotation
            let entity = if let Some(res) =
                alternate_source_rotation
                    .iter()
                    .find_map(|(entity, alternate)| {
                        if alternate.0 == *net_entity_id {
                            Some(entity)
                        } else {
                            None
                        }
                    }) {
                res
            } else {
                entity
            };

            let Ok(transform) = transform_query.get(entity) else {
                continue;
            };
            network_rotation.0 = transform.rotation;
        } else {
            // which entity to apply the network rotation too
            let entity = if let Some(target_entity) =
                alternate_target_rotation
                    .iter()
                    .find_map(|(transform, alternate)| {
                        if alternate.0 == *net_entity_id {
                            Some(transform)
                        } else {
                            None
                        }
                    }) {
                target_entity
            } else {
                entity
            };

            let Ok(mut transform) = transform_query.get_mut(entity) else {
                continue;
            };
            if sync_rotation.linear_interpolation {
                let current = transform.rotation;
                let slerp_factor = 20. * time.delta_secs();
                let slerp_factor_clamped = slerp_factor.clamp(0.0, 1.0);

                let new_rotation = current.slerp(network_rotation.0, slerp_factor_clamped);

                transform.rotation = sync_rotation.apply_rotation_locks(&new_rotation);
            } else {
                transform.rotation = sync_rotation.apply_rotation_locks(&network_rotation.0);
            }
        }
    }
}

/// Ensures all required components are present on entities with SyncPosition component.
fn add_required_components_position(
    mut commands: Commands,
    query: Query<Entity, Added<SyncPosition>>,
) {
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

/// Ensures all required components are present on entities with SyncRotation component.
fn add_required_components_rotation(
    mut commands: Commands,
    query: Query<Entity, Added<SyncRotation>>,
) {
    for entity in query {
        info!(
            ?entity,
            "Adding required components to new SyncRotation entity"
        );
        commands
            .entity(entity)
            .insert((NetworkRotation::default(), Transform::default()));
    }
}
