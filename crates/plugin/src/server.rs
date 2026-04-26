use std::{
    io,
    net::{SocketAddr, UdpSocket},
};

use bevy::prelude::*;

use crate::{
    NEW_CONNECTION_MESSAGE,
    network::{bind_server, receive_bytes_from_socket},
};

/// Trigger this Event to start a local server
#[derive(Event)]
pub struct StartServer {
    /// The port on which the server should be started
    pub port: u16,
}

#[derive(Resource)]
pub struct CurrentServerSocket(pub UdpSocket);

/// Stores all connected clients so we know to which address to send data to
#[derive(Resource, Default)]
pub struct ConnectedClients(pub Vec<SocketAddr>);

pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConnectedClients>();

        app.add_observer(handle_start_server);

        app.add_systems(Update, handle_server_data);
    }
}

// now the socket only receives once. so the recv needs to happen in Update system
pub fn handle_start_server(event: On<StartServer>, mut commands: Commands) {
    info!("Handling StartServer event");
    let socket = bind_server(event.port);
    commands.insert_resource(CurrentServerSocket(socket));
}

/// Receive bytes from the current server socket.
/// The server will send all received bytes to all connected clients
pub fn handle_server_data(
    current_server_socket: If<Res<CurrentServerSocket>>,
    mut connected_clients: ResMut<ConnectedClients>,
) {
    let mut is_first_sent = false;
    let bytes = receive_bytes_from_socket(&current_server_socket.0.0);

    let Some((bytes, src_address)) = bytes else {
        return;
    };

    if !connected_clients.0.contains(&src_address) {
        info!("Received data from a new client, adding it to ConnectedClients");
        connected_clients.0.push(src_address);
        is_first_sent = true;
    }

    // we dont need to send the first bytes from a new client to other clients, currently thats just
    // a mock message so we can register the new client
    if bytes.is_empty() || is_first_sent {
        return;
    }

    for connected_client in &connected_clients.0 {
        // we of course dont need to send back the data we just received
        if *connected_client == src_address {
            continue;
        }
        let res = current_server_socket.0.0.send_to(&bytes, connected_client);
        match res {
            Ok(count_b) => {
                info!("Sent {} bytes to {}", count_b, connected_client);
                info!("Bytes sent are: {:?}", bytes);
                println!();
            }
            Err(error) => {
                error!("Couldnt sent bytes: {}", error);
            }
        }
    }
}
