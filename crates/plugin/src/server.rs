use std::{io, net::UdpSocket};

use bevy::prelude::*;

use crate::network::bind_server;

/// This message gets written whenever a new server is listening
#[derive(Message)]
pub struct ServerListening(pub u16);

#[derive(Event)]
pub struct ConnectToServer {
    pub server_url: String,
    pub port: u16,
}

/// Trigger this Event to start a local server
#[derive(Event)]
pub struct StartServer {
    /// The port on which the server should be started
    pub port: u16,
}

#[derive(Resource)]
struct CurrentServerSocket(pub UdpSocket);

// now the socket only receives once. so the recv needs to happen in Update system
pub fn handle_start_server(event: On<StartServer>, mut commands: Commands) {
    debug!("Handling StartServer event");
    let socket = bind_server(event.port);
    commands.insert_resource(CurrentServerSocket(socket));
}

fn receive_bytes_from_server_socket(socket: &UdpSocket) -> Vec<u8> {
    // data received this system tick
    let mut data_received: Vec<u8> = Vec::new();

    let mut buf = [0; 10];
    let num_bytes_read = loop {
        match socket.recv(&mut buf) {
            Ok(n) => {
                for byte in &buf[0..n] {
                    data_received.push(*byte);
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

pub fn handle_server_data(mut current_server_socket: If<ResMut<CurrentServerSocket>>) {
    let bytes = receive_bytes_from_server_socket(&current_server_socket.0.0);
    info!("Bytes received this tick: {:?}", bytes);
}
