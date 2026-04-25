use std::{
    io,
    net::{SocketAddr, UdpSocket},
};

use bevy::prelude::*;

use crate::network::bind_server;

/// Trigger this Event to start a local server
#[derive(Event)]
pub struct StartServer {
    /// The port on which the server should be started
    pub port: u16,
}

#[derive(Resource)]
pub struct CurrentServerSocket(pub UdpSocket);

#[derive(Resource)]
pub struct ConnectedClients(pub Vec<SocketAddr>);

// now the socket only receives once. so the recv needs to happen in Update system
pub fn handle_start_server(event: On<StartServer>, mut commands: Commands) {
    debug!("Handling StartServer event");
    let socket = bind_server(event.port);
    commands.insert_resource(CurrentServerSocket(socket));
}

fn receive_bytes_from_server_socket(socket: &UdpSocket) -> &[u8] {
    // data received this system tick
    let data_received: &mut [u8] = &mut [];

    let mut buf = [0; 10];
    let num_bytes_read = loop {
        match socket.recv(&mut buf) {
            Ok(n) => {
                for byte in &buf[0..n] {
                    data_received[data_received.len()] = *byte;
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                break 0;
            }
            Err(e) => panic!("encountered IO error: {e}"),
        }
    };
    info!("num num_bytes_read: {}", num_bytes_read);
    info!("bytes read: {:?}", data_received);

    data_received
}

/// Receive bytes from the current server socket. Clients send data to the server, and server then
/// sends these bytes to all connected clients
pub fn handle_server_data(
    mut current_server_socket: If<ResMut<CurrentServerSocket>>,
    connected_clients: Res<ConnectedClients>,
) {
    let bytes = receive_bytes_from_server_socket(&current_server_socket.0.0);
    info!("Bytes received this tick: {:?}", bytes);
    let res = current_server_socket.0.0.send(bytes);
    match res {
        Ok(count_b) => {
            info!("Sent {} bytes to ?", count_b);
        }
        Err(error) => {
            error!("Couldnt sent bytes to ?: {}", error);
        }
    }
}
