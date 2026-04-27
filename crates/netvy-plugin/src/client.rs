use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;
use shared::util::receive_bytes_from_socket;

use crate::{
    ComponentRegistry,
    net_entity::{NetEntityId, NetEntityMapping},
    network::connect_to_server,
    util::{extract_component_type_id, extract_net_entity_id, parse_connect_to_server},
};

/// Trigger this event on the client to connect to a server
#[derive(Event)]
pub struct ConnectToServer {
    pub server_url: String,
    pub port: u16,
}

/// The socket of the current client
#[derive(Resource)]
pub struct CurrentClientSocket(pub UdpSocket);

/// Add this plugin on the client
pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_connect_trigger);

        app.add_message::<NewNetEntityMessage>();

        app.add_systems(
            Update,
            (handle_data_client_socket, handle_new_net_entity_message),
        );
    }
}

// we should have a message that the server receives that tells the server its a new connect from a
// client
fn handle_connect_trigger(trigger: On<ConnectToServer>, mut commands: Commands) {
    debug!("Handling ConnectToServer event");
    let address = parse_connect_to_server(trigger.event());

    let client_socket = connect_to_server(address);

    info!("Sending new connect message to server!");
    client_socket
        .send(&[1])
        .expect("Can send new connect message to server");

    commands.insert_resource(CurrentClientSocket(client_socket));
}

fn handle_data_client_socket(world: &mut World) {
    let Some((bytes, _)) = get_bytes_from_client_socket(world) else {
        return;
    };

    // info!(
    //     "Received data from server, applying it to our world using the apply_fn from our ComponentRegistry"
    // );

    // first byte is internal type id
    let Some(internal_type_id_bytes) = extract_component_type_id(&bytes) else {
        error!("Couldnt extract internal component type id");
        return;
    };

    let apply_fn = {
        let Some(component_registry) = world.get_resource::<ComponentRegistry>() else {
            return;
        };
        let Some(apply_fn) = component_registry.apply.get(&internal_type_id_bytes) else {
            return;
        };
        *apply_fn
    };

    let Some(extracted_net_entity_id) = extract_net_entity_id(&bytes) else {
        warn!(
            "Received datagram that doesnt contain a NetEntityId {:?}",
            bytes
        );
        return;
    };

    if let Some(existing_entity) = world
        .resource::<NetEntityMapping>()
        .0
        .get(&extracted_net_entity_id)
    {
        apply_fn(world, *existing_entity, &bytes);
    } else {
        // NOTE: This will mean this current component update wont be done, only spawning the new
        // entity, but the next one will be
        world.write_message(NewNetEntityMessage(extracted_net_entity_id));
    }
}

#[derive(Message)]
pub struct NewNetEntityMessage(pub NetEntityId);

pub fn handle_new_net_entity_message(
    mut commands: Commands,
    mut message_reader: MessageReader<NewNetEntityMessage>,
    mut net_entity_mapping: ResMut<NetEntityMapping>,
) {
    for message in message_reader.read() {
        info!("Spawning local entity for new NetEntityId!");
        let entity_id = commands.spawn_empty().id();
        net_entity_mapping.0.insert(message.0.clone(), entity_id);
    }
}

fn get_bytes_from_client_socket(world: &World) -> Option<(Vec<u8>, SocketAddr)> {
    let socket = world.resource::<CurrentClientSocket>();
    receive_bytes_from_socket(&socket.0)
}
