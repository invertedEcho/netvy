use std::net::UdpSocket;

use bevy::prelude::*;

use crate::{network::connect_to_server, util::parse_connect_to_server};

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
    }
}

fn handle_connect(event: On<ConnectToServer>, mut commands: Commands) {
    debug!("Handling ConnectToServer event");
    let address = parse_connect_to_server(event.event());

    let socket = connect_to_server(address);

    commands.insert_resource(CurrentClientSocket(socket));
}
