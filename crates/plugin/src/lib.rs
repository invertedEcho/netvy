use std::{io, thread};

use bevy::prelude::*;
use log::debug;

use crate::{
    network::{bind_server, connect_to_server},
    util::parse_connect_to_server,
};

mod network;
mod server;
mod util;

#[derive(Clone, Copy)]
pub enum PluginType {
    Client,
    Server,
}

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
pub struct GlobalConfiguration {
    plugin_type: PluginType,
}

pub struct BevyMultiplayerFrameworkPlugin(pub PluginType);

impl Plugin for BevyMultiplayerFrameworkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalConfiguration {
            plugin_type: self.0,
        });
        app.add_observer(handle_connect)
            .add_observer(handle_start_server);
    }
}

fn handle_connect(event: On<ConnectToServer>) {
    debug!("Handling ConnectToServer event");
    let address = parse_connect_to_server(event.event());
    connect_to_server(address);
}

#[derive(Resource)]
struct CurrentServerSocket(pub UdpSocket);

// now the socket only receives once. so the recv needs to happen in Update system
fn handle_start_server(event: On<StartServer>) {
    debug!("Handling StartServer event");
    let socket = bind_server(event.port);

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
}

fn read_from_server_socket() {}
