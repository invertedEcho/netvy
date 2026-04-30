use std::any::{Any, TypeId};

use crate::{
    client::{ClientPlugin, CurrentClientSocket, handle_new_sync_entities},
    net_entity::{
        EntityType, NetEntityId, NetEntityMapping, NextTemporaryNetId,
        get_net_entity_for_local_entity,
    },
    server::{NextNetEntityId, ServerPlugin},
    util::COMPONENT_UPDATE_BYTE_HEADER,
};
use bevy::{platform::collections::HashMap, prelude::*, render::render_resource::encase::internal};
use bincode::{
    Decode, Encode,
    config::{self},
};

pub mod client;
pub mod net_entity;
pub mod network;
pub mod server;
pub mod util;

type ApplyFn = fn(&mut World, Entity, &[u8]);

// We cant use bevys component id, because they are not stable across worlds.
// This is ultiumately what gets sent in the datagram, and then we can lookup the corresponding
// deserialize fn in the `ComponentRegistry`

type ComponentTypeId = u8;

#[derive(Resource, Default)]
struct NextComponentTypeId(pub ComponentTypeId);

// while this allows us to create a mapping for new registered components, if we now actually want
// to know the ComponentTypeId for a type<C>, that wont work. so we also need to store that
// information. we do so by using rusts TypeId. even if this is not stable, it doesnt matter because
// each client has this mapping
// TODO: this is not completely stable. we would need deterministic ID so this is less likely to break
#[derive(Resource, Default)]
struct ComponentRegistry {
    apply: HashMap<ComponentTypeId, ApplyFn>,
    type_id_to_component_type_id: HashMap<TypeId, ComponentTypeId>,
}

#[derive(Resource)]
pub struct AppTypeRes(pub AppType);

#[derive(Clone, Copy)]
pub enum AppType {
    Client,
    Server,
}

/// Add this component to entities of which position (transform.translation) you want to be synced across clients
#[derive(Component)]
pub struct SyncPosition;

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
        app.init_resource::<NetEntityMapping>();
        app.init_resource::<NextTemporaryNetId>();

        app.insert_resource(AppTypeRes(self.0));

        match self.0 {
            AppType::Client => {
                app.add_plugins(ClientPlugin);
            }
            AppType::Server => {
                app.add_plugins(ServerPlugin);
            }
        }

        app.register_component::<InternalSyncPosition>();

        app.add_systems(
            Update,
            (
                add_internal_sync_position_component,
                handle_new_sync_entities,
                apply_internal_sync_position,
            ),
        );

        // TODO: This shouldnt happen if release build.
        // We have this so we can inspect NetEntityId in bevy_inspector_egui
        app.register_type::<NetEntityId>()
            .register_type::<InternalSyncPosition>()
            .register_type::<EntityType>();
    }
}

pub trait AppComponentExt {
    /// Registers the component in the Registry
    /// This component can now be sent over the network.
    fn register_component<C>(&mut self)
    where
        C: Decode<()> + 'static + Component + Encode + std::fmt::Debug;
}

impl AppComponentExt for App {
    fn register_component<C>(&mut self)
    where
        C: Decode<()> + 'static + Component + Encode + std::fmt::Debug,
    {
        let world = self.world_mut();

        let id = {
            let mut next = world.resource_mut::<NextComponentTypeId>();
            let id = next.0;
            next.0 += 1;
            id
        };

        let mut component_id_map = world.resource_mut::<ComponentRegistry>();

        component_id_map.apply.insert(id, |world, entity, bytes| {
            let config = config::standard();

            let component_type_id = bytes[2];

            let (component, _size): (C, usize) = bincode::decode_from_slice(bytes, config).unwrap();

            info!(
                "RECEIVER SIDE. Serialized to bytes: {bytes:?} And back deserialized: {component:?}"
            );

            match world.get_entity_mut(entity) {
                Ok(mut entity_commands) => {
                    entity_commands.insert(component);
                }
                Err(error) => {
                    error!("Failed to apply component update: {error:?}");
                }
            }
        });

        component_id_map
            .type_id_to_component_type_id
            .insert(TypeId::of::<C>(), id);

        self.add_systems(Update, detect_registered_component_change::<C>);

        info!("Registered a new component! {}", std::any::type_name::<C>());
    }
}

fn detect_registered_component_change<C>(
    component_registry: Res<ComponentRegistry>,
    // TODO: the changed filter wont work here. on remote clients, we apply changes to that changed
    // component. that triggers a component change, and it will send that change again.
    changed_entities: Query<(Entity, &C), Changed<C>>,
    client_socket: If<Res<CurrentClientSocket>>,
    net_entity_mapping: Res<NetEntityMapping>,
) where
    C: Component + Encode + std::fmt::Debug + Decode<()>,
{
    for (entity, changed_component) in changed_entities {
        debug!(
            "Synced Entity {entity} has changed component thats registered: {changed_component:?}"
        );

        let serialized_to_bytes =
            bincode::encode_to_vec(changed_component, config::standard()).unwrap();

        let (back_deserialized, _size): (C, usize) =
            bincode::decode_from_slice(&serialized_to_bytes, config::standard()).unwrap();

        info!(
            "SENDER SIDE CHANGED COMPONENT. Serialized to bytes: {serialized_to_bytes:?} And back deserialized: {back_deserialized:?}. Component currently: {changed_component:?}"
        );

        let type_id = changed_component.type_id();

        let component_type_id = component_registry
            .type_id_to_component_type_id
            .get(&type_id)
            .expect("Given Component must be registered");

        let Some(net_entity_id) = get_net_entity_for_local_entity(&net_entity_mapping, entity)
        else {
            debug!(
                "Failed to find NetEntityId for local entity {entity:?}! Skipping sending this update to the server"
            );
            continue;
        };

        let mut data = Vec::new();

        data.extend_from_slice(&[COMPONENT_UPDATE_BYTE_HEADER]);

        data.extend_from_slice(&[net_entity_id.0]);

        // 2 bytes in big endian because thats what rust docs say for networking
        data.extend_from_slice(&[*component_type_id]);

        data.extend_from_slice(&serialized_to_bytes);

        // send data of changed entity / comp to server
        client_socket.0.0.send(&data);
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

// waaaait this would clash with physics... because physics also apply to transform
// but it would be fine if we only apply this to transform of other clients, and physics only run on
// local player / client -> i guess we could just disable this if the user wants to run physics on
// all entities?
/// Runs on both server and client.
/// wait we want to apply internal sync position to transform?
/// but we also want to apply changes to transform to internal sync position...
/// e.g. on our client, we change the transform directly, and want that change to be visible on
/// internal sync position.
/// so really this system should depend on whether the entity is the local client, or a remote client
fn apply_internal_sync_position(
    mut commands: Commands,
    query: Query<(
        Entity,
        Option<&mut Transform>,
        &mut InternalSyncPosition,
        &EntityType,
    )>,
) {
    for (entity, transform, mut internal_sync_position, entity_type) in query {
        let x = internal_sync_position.x;
        let y = internal_sync_position.y;
        let z = internal_sync_position.z;

        if let Some(mut transform) = transform {
            match entity_type {
                EntityType::Local => {
                    internal_sync_position.x = transform.translation.x;
                    internal_sync_position.y = transform.translation.y;
                    internal_sync_position.z = transform.translation.z;
                }
                EntityType::Remote => {
                    transform.translation = vec3(x, y, z);
                }
            }
        } else {
            commands.entity(entity).insert(Transform::from_xyz(x, y, z));
        }
    }
}
