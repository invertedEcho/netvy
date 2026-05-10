use std::{
    any::{Any, TypeId},
    net::{SocketAddr, UdpSocket},
};

use crate::{
    client::{ClientPlugin, handle_new_sync_entities},
    datagram::build_component_update_datagram,
    net_entity::{NetEntityId, NetEntityType, NextTemporaryNetId},
    server::{NextNetEntityId, ServerPlugin},
};
use bevy::{platform::collections::HashMap, prelude::*};
use bincode::{
    Decode, Encode,
    config::{self, BigEndian, Configuration},
    error::DecodeError,
};

// TODO: At some point we probably want to re-export specific stuff instead of everything
pub mod client;
mod datagram;
pub mod net_entity;
pub mod network;
pub mod server;
mod util;

const BINCODE_CONFIG: Configuration<BigEndian> = config::standard().with_big_endian();

pub enum SyncMode {
    /// Sends component updates every x seconds (right now even if unchanged)
    FixedRate(f32),
    /// Sends component updates whenever the component changes
    OnChange,
}

impl Default for SyncMode {
    fn default() -> Self {
        Self::FixedRate(0.05)
    }
}

/// The socket of the current running instance (server or client)
#[derive(Resource)]
pub struct CurrentSocket(pub UdpSocket);

/// Stores the sequence number of component updates for each net entity and a corresponding component type id
/// Used to ensure only newer updates are applied as UDP is unordered
#[derive(Resource, Clone, Reflect, Default)]
pub struct UpdateSequence(pub HashMap<(NetEntityId, ComponentTypeId), u32>);

// returns whether applying the update was succesful
type ApplyFn = fn(&mut EntityCommands, &[u8]) -> bool;

// We cant use bevys component id, because they are not stable across worlds.
// This is what gets sent in the datagram, and then we can lookup the corresponding
// deserialize fn in the `ComponentRegistry`

#[derive(Resource, Default)]
struct NextComponentTypeId(pub ComponentTypeId);

type ComponentTypeId = u8;

// while this allows us to create a mapping for new registered components, if we now actually want
// to know the ComponentTypeId for a type<C>, that wont work. so we also need to store that
// information. we do so by using rusts TypeId. even if this is not stable, it doesnt matter because
// each client has this mapping
// TODO: this is not completely stable. we would need deterministic ID so this is less likely to break
#[derive(Resource, Default)]
struct ComponentRegistry {
    apply: HashMap<ComponentTypeId, ApplyFn>,
    type_id_to_component_type_id: HashMap<TypeId, ComponentTypeId>,
    timer: HashMap<ComponentTypeId, Timer>,
}

struct FailedSentComponentUpdate {
    entity: Entity,
    component_bytes: Vec<u8>,
    component_type_id: u8,
}

/// Stores failed component updates that could not be sent to the server.
/// For example, an entity with a registered component changed locally, but that entity doesn't have a
/// NetEntityId yet.
#[derive(Resource, Default)]
struct FailedSentComponentUpdates(pub Vec<FailedSentComponentUpdate>);

#[derive(Resource, Clone, Copy, PartialEq)]
pub enum AppType {
    Client,
    Server,
}

/// Add this component to entities of which position (transform.translation) you want to be synced across clients
#[derive(Component, Encode, Decode)]
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

#[derive(Debug)]
struct ComponentUpdate {
    net_entity_id: NetEntityId,
    component_type_id: ComponentTypeId,
    component_bytes: Vec<u8>,
    update_sequence: u32,
}

/// Add this component to entities that should be synced across clients.
/// This component is the bare minimum and always required for an entity to be taken into
/// consideration by netvy.
/// Upon adding this component, netvy will add a NetEntityId component into this entity, that
/// identifies the entity across all clients. The NetEntityId will always be the same across clients.
#[derive(Component)]
pub struct SyncEntity;

// Because vec3 doesnt implement bincode::encode and bincode::decode, we use three f32 instead
#[derive(Component, Encode, Decode, Reflect, Debug)]
struct InternalSyncPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Add this plugin and specify whether this is a client or a server
/// Depending on the given `AppType`, specific systems will run
pub struct NetvyPlugin(pub AppType);

impl Plugin for NetvyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComponentRegistry>();

        app.init_resource::<NextComponentTypeId>();
        app.init_resource::<NextNetEntityId>();
        app.init_resource::<NextTemporaryNetId>();
        app.init_resource::<FailedSentComponentUpdates>();
        app.init_resource::<UpdateSequence>();

        app.insert_resource(self.0);

        match self.0 {
            AppType::Client => {
                app.add_plugins(ClientPlugin);
            }
            AppType::Server => {
                app.add_plugins(ServerPlugin);
            }
        }

        app.register_component::<InternalSyncPosition>();
        app.register_component_with_sync_mode::<SyncPosition>(SyncMode::OnChange);

        app.add_systems(
            Update,
            (
                add_entity_type_to_sync_entities,
                add_internal_sync_position_component,
                handle_new_sync_entities,
                handle_failed_sent_component_updates,
                handle_send_interval_timer,
            ),
        );

        if cfg!(debug_assertions) {
            app.register_type::<NetEntityId>()
                .register_type::<InternalSyncPosition>()
                .register_type::<NetEntityType>()
                .register_type::<UpdateSequence>();
        }
    }
}

pub trait AppComponentExt {
    /// Registers the component in the Registry
    /// This component can now be sent over the network.
    /// This uses the default SyncMode.
    fn register_component<C>(&mut self)
    where
        C: Decode<()> + 'static + Component + Encode;

    /// If you want to specify how frequent updates should be done for the specified component, you
    /// may do so by using the paramter `sync_mode`
    fn register_component_with_sync_mode<C>(&mut self, sync_mode: SyncMode)
    where
        C: Decode<()> + 'static + Component + Encode;
}

impl AppComponentExt for App {
    fn register_component<C>(&mut self)
    where
        C: Decode<()> + 'static + Component + Encode,
    {
        self.register_component_with_sync_mode::<C>(SyncMode::default());
    }

    fn register_component_with_sync_mode<C>(&mut self, sync_mode: SyncMode)
    where
        C: Decode<()> + 'static + Component + Encode,
    {
        let world = self.world_mut();

        let component_type_id = {
            let mut next = world.resource_mut::<NextComponentTypeId>();
            let id = next.0;
            next.0 += 1;
            id
        };

        let mut component_registry = world.resource_mut::<ComponentRegistry>();

        component_registry
            .apply
            .insert(component_type_id, |entity_commands, bytes| {
                let Ok((component, _size)): Result<(C, usize), DecodeError> =
                    bincode::decode_from_slice(bytes, BINCODE_CONFIG)
                else {
                    warn!("Couldnt decode bytes");
                    return false;
                };

                entity_commands.insert(component);
                true
            });

        component_registry
            .type_id_to_component_type_id
            .insert(TypeId::of::<C>(), component_type_id);

        match sync_mode {
            SyncMode::FixedRate(fixed_rate) => {
                component_registry.timer.insert(
                    component_type_id,
                    Timer::from_seconds(fixed_rate, TimerMode::Repeating),
                );
                self.add_systems(Update, send_component_updates_fixed_rate::<C>);
            }
            SyncMode::OnChange => {
                self.add_systems(Update, detect_registered_component_change::<C>);
            }
        }

        info!(
            "Registered a new component! {}. component_type_id: {component_type_id}",
            std::any::type_name::<C>()
        );
    }
}

fn handle_send_interval_timer(time: Res<Time>, mut component_registry: ResMut<ComponentRegistry>) {
    for timer in component_registry.timer.values_mut() {
        timer.tick(time.delta());
    }
}

fn send_component_updates_fixed_rate<C>(
    component_registry: Res<ComponentRegistry>,
    entities: Query<(Entity, &C, Option<&NetEntityId>, &NetEntityType)>,
    mut update_sequence: ResMut<UpdateSequence>,
    current_socket: If<Res<CurrentSocket>>,
    mut failed_sent_component_updates: ResMut<FailedSentComponentUpdates>,
) where
    C: Component + Encode + Decode<()>,
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

        let component_bytes = bincode::encode_to_vec(component, BINCODE_CONFIG).unwrap();

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

fn detect_registered_component_change<C>(
    component_registry: Res<ComponentRegistry>,
    changed_entities: Query<(Entity, &C, Option<&NetEntityId>, Option<&NetEntityType>), Changed<C>>,
    current_socket: If<Res<CurrentSocket>>,
    mut failed_sent_component_updates: ResMut<FailedSentComponentUpdates>,
    mut update_sequence: ResMut<UpdateSequence>,
) where
    C: Component + Encode + Decode<()>,
{
    for (entity, changed_component, maybe_net_entity_id, maybe_net_entity_type) in changed_entities
    {
        let component_bytes = bincode::encode_to_vec(changed_component, BINCODE_CONFIG).unwrap();

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

// All entities that have SyncEntity component are local entities
fn add_entity_type_to_sync_entities(
    mut commands: Commands,
    query: Query<Entity, Added<SyncEntity>>,
) {
    for entity in query {
        commands.entity(entity).insert(NetEntityType::Local);
    }
}

fn handle_failed_sent_component_updates(
    mut resource: ResMut<FailedSentComponentUpdates>,
    entities: Query<&NetEntityId>,
    current_socket: If<Res<CurrentSocket>>,
    mut update_sequence: ResMut<UpdateSequence>,
) {
    resource.0.retain(|failed_component_update| {
        let Ok(net_entity_id) = entities.get(failed_component_update.entity) else {
            info!("still cant apply failed component update, no matching entity");
            return true;
        };

        let current_update_sequence = get_or_create_mut_update_sequence_number(
            &mut update_sequence,
            *net_entity_id,
            failed_component_update.component_type_id,
        );

        *current_update_sequence += 1;

        let data = build_component_update_datagram(
            &failed_component_update.component_bytes,
            failed_component_update.component_type_id,
            net_entity_id,
            *current_update_sequence,
        );

        // dont retain if sending was succesful
        !current_socket.0.0.send(&data).is_ok()
    });
}

fn get_or_create_mut_update_sequence_number<'a>(
    update_sequence: &'a mut UpdateSequence,
    net_entity_id: NetEntityId,
    component_type_id: ComponentTypeId,
) -> &'a mut u32 {
    update_sequence
        .0
        .entry((net_entity_id, component_type_id))
        .or_insert(0)
}
