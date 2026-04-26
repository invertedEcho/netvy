use std::net::UdpSocket;

use bevy::prelude::*;

use crate::{
    ComponentRegistry, NEW_CONNECTION_MESSAGE,
    network::{connect_to_server, receive_bytes_from_socket},
    util::parse_connect_to_server,
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

    info!("Send new connect message to server!");
    client_socket
        .send(&NEW_CONNECTION_MESSAGE)
        .expect("Can send new connect message to server");

    commands.insert_resource(CurrentClientSocket(client_socket));
}

fn handle_data_client_socket(
    client_socket: If<Res<CurrentClientSocket>>,
    component_registry: Res<ComponentRegistry>,
) {
    let res = receive_bytes_from_socket(&client_socket.0.0);

    // now interpret this data...
    // first we have to get the deserialize fn by using our *fancy fancy* component registry
}
