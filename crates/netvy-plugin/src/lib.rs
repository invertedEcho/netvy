use std::any::{Any, TypeId};

use crate::{
    client::{ClientPlugin, CurrentClientSocket},
    net_entity::{NetEntityMapping, add_net_entity_id, get_net_entity_for_local_entity},
};
use bevy::{platform::collections::HashMap, prelude::*};
use bincode::{
    Decode, Encode,
    config::{self},
};
use netvy_server::{NextNetEntityId, ServerPlugin};

// re-export
pub use netvy_server::StartServer;

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
#[derive(Component, Encode, Decode)]
struct InternalSyncPosition(pub f32, pub f32, pub f32);

/// Add this plugin and specify whether this is a client or a server
/// Depending on the given `AppType`, specific systems will run
pub struct NetvyPlugin(pub AppType);

impl Plugin for NetvyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComponentRegistry>();

        app.init_resource::<NextComponentTypeId>();
        app.init_resource::<NextNetEntityId>();
        app.init_resource::<NetEntityMapping>();

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
            (add_internal_sync_position_component, add_net_entity_id),
        );
    }
}

pub trait AppComponentExt {
    /// Registers the component in the Registry
    /// This component can now be sent over the network.
    fn register_component<C>(&mut self)
    where
        C: Decode<()> + 'static + Component + Encode;
}

impl AppComponentExt for App {
    fn register_component<C>(&mut self)
    where
        C: Decode<()> + 'static + Component + Encode,
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
            let config = config::standard().with_big_endian();
            let (component, _): (C, usize) = bincode::decode_from_slice(bytes, config).unwrap();

            world.entity_mut(entity).insert(component);
        });

        component_id_map
            .type_id_to_component_type_id
            .insert(TypeId::of::<C>(), id);

        self.add_systems(Update, detect_registered_component_change::<C>);

        info!("Registered a new component!");
    }
}

// This should happen on the client. The client detects changes to registered components and send
// the data to the server, so the server can send the data to all other connected clients
fn detect_registered_component_change<C>(
    component_registry: Res<ComponentRegistry>,
    changed_entities: Query<(Entity, &C), Changed<C>>,
    client_socket: Option<Res<CurrentClientSocket>>,
    net_entity_mapping: Res<NetEntityMapping>,
) where
    C: Component + Encode,
{
    if let Some(client_socket) = client_socket {
        for (entity, changed_component) in changed_entities {
            let serialized_to_bytes =
                bincode::encode_to_vec(changed_component, config::standard()).unwrap();

            let type_id = changed_component.type_id();

            let component_type_id = component_registry
                .type_id_to_component_type_id
                .get(&type_id)
                .expect("Given Component must be registered");

            let mut data = Vec::new();

            let Some(net_entity_id) = get_net_entity_for_local_entity(&net_entity_mapping, entity)
            else {
                warn!(
                    "Failed to find NetEntityId for local entity {entity:?}! Skipping sending this update to the server"
                );
                continue;
            };

            data.extend_from_slice(&net_entity_id.0.to_be_bytes());

            // 2 bytes in big endian because thats what rust docs say for networking
            data.extend_from_slice(&component_type_id.to_be_bytes());

            data.extend_from_slice(&serialized_to_bytes);

            // send data of changed entity / comp to server
            let result = client_socket.0.send(&data);
            debug!("{:?}", result);
        }
    }
}

fn add_internal_sync_position_component(
    query: Query<(Entity, &Transform), Added<SyncPosition>>,
    mut commands: Commands,
) {
    for (entity, transform) in query {
        let position = transform.translation;
        commands
            .entity(entity)
            .insert(InternalSyncPosition(position.x, position.y, position.z));
    }
}

// waaaait this would clash with physics... because physics also apply to transform
// but it would be fine if we only apply this to transform of other clients, and physics only run on
// local player / client -> i guess we could just disable this if the user wants to run physics on
// all entities?
fn apply_internal_sync_to_transform() {}
