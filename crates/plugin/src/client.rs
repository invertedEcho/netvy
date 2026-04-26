use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;

use crate::{
    ComponentRegistry, NetEntityMapping,
    network::{connect_to_server, receive_bytes_from_socket},
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
        app.add_observer(handle_connect);

        app.add_systems(Update, handle_data_client_socket);
    }
}

// we should have a message that the server receives that tells the server its a new connect from a
// client
fn handle_connect(trigger: On<ConnectToServer>, mut commands: Commands) {
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

    info!(
        "Received data from server, applying it to our world using the apply_fn from our ComponentRegistry"
    );

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

    let entity = {
        let net_entity_mapping = world.resource::<NetEntityMapping>();
        match net_entity_mapping.0.get(&extracted_net_entity_id) {
            // it already exists, no need to spawn the entity
            Some(entity) => *entity,
            None => {
                info!("Received datagram with new entity! Spawning new entity!");
                // if the entity doesnt exist, we need to spawn it first
                // TODO: even if this is the completely wrong place for that...
                world.spawn(extracted_net_entity_id).id()
            }
        }
    };

    apply_fn(world, entity, &bytes);
}

fn get_bytes_from_client_socket(world: &World) -> Option<(Vec<u8>, SocketAddr)> {
    let socket = world.resource::<CurrentClientSocket>();
    receive_bytes_from_socket(&socket.0)
}
