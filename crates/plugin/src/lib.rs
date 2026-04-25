use std::any::{Any, TypeId};

use crate::{
    client::{ClientPlugin, CurrentClientSocket},
    server::ServerPlugin,
};
use bevy::{platform::collections::HashMap, prelude::*};
use bincode::{Decode, Encode, config};

pub mod client;
pub mod network;
pub mod server;
pub mod util;

// we need to know to which entity to apply a change to across clients. bevy entity ids are not
// stable across worlds.
// so we need a network entity id.
// each client/server has a mapping for a given network entity id to its local entity id
struct NetEntityId(u64);

#[derive(Resource)]
struct NetEntityMapping(HashMap<NetEntityId, Entity>);

// we need to have a uniform type, because just using generic wont work. thats why we use Any here
// we also need to use Box<> because we need to have same size for each item in the collection, so
// Box gives us the pointer to the data on the heap
type DeserializeFn = for<'a> fn(&[u8]) -> Box<dyn Any>;

// We cant use bevys component id, because they are not stable across worlds.
// This is ultiumately what gets sent in the datagram, and then we can lookup the corresponding
// deserialize fn in the `ComponentRegistry`

type ComponentTypeId = u8;

#[derive(Resource, Default)]
struct NextComponentTypeId(pub u8);

// while this allows us to create a mapping for new registered components, if we now actually want
// to know the ComponentTypeId for a type<C>, that wont work. so we also need to store that
// information. we do so by using rusts TypeId. even if this is not stable, it doesnt matter because
// each client has this mapping
// TODO: this is not completely stable. we would need deterministic ID so this is less likely to break
#[derive(Resource, Default)]
struct ComponentRegistry {
    deserialize: HashMap<ComponentTypeId, DeserializeFn>,
    type_id_to_component_type_id: HashMap<TypeId, ComponentTypeId>,
}

#[derive(Resource)]
pub struct AppTypeRes(pub AppType);

#[derive(Clone, Copy)]
pub enum AppType {
    Client,
    Server,
}

/// Add this plugin and specify whether this is a client or a server
/// Depending on the given `AppType`, specific systems will run
pub struct BevyMultiplayerFrameworkPlugin(pub AppType);

impl Plugin for BevyMultiplayerFrameworkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComponentRegistry>();

        app.init_resource::<NextComponentTypeId>();

        app.insert_resource(AppTypeRes(self.0));

        match self.0 {
            AppType::Client => {
                app.add_plugins(ClientPlugin);
            }
            AppType::Server => {
                app.add_plugins(ServerPlugin);
            }
        }
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

        component_id_map.deserialize.insert(id, |bytes| {
            let config = config::standard();
            let (decoded, _): (C, usize) = bincode::decode_from_slice(bytes, config).unwrap();
            Box::new(decoded)
        });
        component_id_map
            .type_id_to_component_type_id
            .insert(TypeId::of::<C>(), id);

        self.add_systems(Update, detect_registered_component_change::<C>);
    }
}

// This should happen on the client. The client detects changes to registered components and send
// the data to the server, so the server can send the data to all other connected clients
fn detect_registered_component_change<C>(
    component_registry: Res<ComponentRegistry>,
    changed_comps: Query<&C, Changed<C>>,
    client_socket: Res<CurrentClientSocket>,
) where
    C: Component + Encode,
{
    for changed_comp in changed_comps {
        let serialized_to_bytes = bincode::encode_to_vec(changed_comp, config::standard()).unwrap();

        let type_id = changed_comp.type_id();

        let component_type_id = component_registry
            .type_id_to_component_type_id
            .get(&type_id)
            .expect("Given Component must be registered");

        let mut data = Vec::new();

        // 2 bytes in big endian because thats what rust docs say for networking
        data.extend_from_slice(&component_type_id.to_be_bytes());

        data.extend_from_slice(&serialized_to_bytes);

        // send data of changed entity / comp to server
        let result = client_socket.0.send(&data);
        debug!("{:?}", result);
    }
}
