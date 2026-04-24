use std::any::Any;

use crate::{
    network::connect_to_server,
    server::{ConnectToServer, handle_server_data, handle_start_server},
    util::parse_connect_to_server,
};
use bevy::{ecs::component::ComponentId, platform::collections::HashMap, prelude::*};
use bincode::{Decode, Encode, config};
use log::debug;

pub mod network;
pub mod protocol;
pub mod server;
pub mod util;

#[derive(Clone, Copy)]
pub enum PluginType {
    Client,
    Server,
}

#[derive(Resource)]
pub struct GlobalConfiguration {
    plugin_type: PluginType,
}

pub struct BevyMultiplayerFrameworkPlugin(pub PluginType);

#[derive(Component, Decode, Encode)]
struct TestComponent(pub f32);

impl Plugin for BevyMultiplayerFrameworkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NextComponentKeyId(0));
        app.insert_resource(ComponentIdMap(HashMap::new()));

        app.insert_resource(GlobalConfiguration {
            plugin_type: self.0,
        });

        app.add_observer(handle_connect)
            .add_observer(handle_start_server);

        // app.add_systems(Update, handle_server_data);
        app.register_component::<TestComponent>();
    }
}

fn handle_connect(event: On<ConnectToServer>) {
    debug!("Handling ConnectToServer event");
    let address = parse_connect_to_server(event.event());
    connect_to_server(address);
}

#[derive(Resource)]
struct NextComponentKeyId(pub usize);

// we want to be able to create our own components and register them in this plugin. this way, we
// know what type of component we are receiving by using the key from this map and looking at the
// corresponding bits from a datagram

// we need to have a uniform type, because just using generic wont work. thats why we use Any here
// we also need to use Box<> because this data needs to be stored on the heap because we cant know
// the size at compile time
type DeserializeFn = for<'a> fn(&[u8]) -> Box<dyn Any>;

#[derive(Resource)]
struct ComponentIdMap(pub HashMap<ComponentId, DeserializeFn>);

pub trait AppComponentExt {
    /// Registers the component in the Registry
    /// This component can now be sent over the network.
    fn register_component<C>(&mut self)
    where
        C: Decode<()> + 'static + Component;
}

impl AppComponentExt for App {
    fn register_component<C>(&mut self)
    where
        C: Decode<()> + 'static + Component,
    {
        let component_id = self.world_mut().register_component::<C>();

        let mut component_id_map = self.world_mut().resource_mut::<ComponentIdMap>();

        component_id_map.0.insert(component_id, |bytes| {
            let config = config::standard();
            let decoded: (C, usize) = bincode::decode_from_slice(bytes, config).unwrap();
            Box::new(decoded)
        });
        self.add_systems(Update, detect_registered_component_change::<C>);
    }
}

fn detect_registered_component_change<C>(
    component_id_map: Res<ComponentIdMap>,
    changed_comps: Query<Entity, Changed<C>>,
) where
    C: Component,
{
    for changed_comp in changed_comps {
        info!("HELL YEAH IT WORKS");
    }
}

// using Any gives us the advantage that we can use downcast, if we guess the correct type of that
// value. which in our case we do
/*
let value: Box<dyn Any> = Box::new(5u32);

if let Ok(v) = value.downcast::<u32>() {
    println!("{}", v);
}
*/
