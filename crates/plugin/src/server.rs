use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;

use crate::{
    net_entity::{
        CONFIRM_NEW_NET_ENTITY_BYTE_HEADER, NetEntityId, REQUEST_NEW_NET_ENTITY_BYTE_HEADER,
    },
    util::{bind_socket, receive_bytes_from_socket},
};

#[derive(Resource, Default)]
pub struct NextNetEntityId(pub u8);

#[derive(Resource)]
pub struct CurrentServerSocket(pub UdpSocket);

/// Stores all connected clients so we know to which address to send data to
#[derive(Resource, Default)]
pub struct ConnectedClients(pub Vec<SocketAddr>);

/// Trigger this Event to start a local server
#[derive(Event)]
pub struct StartServer {
    /// The port on which the server should be started
    pub port: u16,
}

pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConnectedClients>();

        app.add_observer(handle_start_server);

        app.add_systems(Update, handle_server_data);
    }
}

/// Receive bytes from the current server socket.
/// The server will send all received bytes to all connected clients
pub fn handle_server_data(
    mut commands: Commands,
    current_server_socket: If<Res<CurrentServerSocket>>,
    mut connected_clients: ResMut<ConnectedClients>,
    mut next_net_entity_id: ResMut<NextNetEntityId>,
) {
    let bytes = receive_bytes_from_socket(&current_server_socket.0.0);

    let Some((bytes, src_address)) = bytes else {
        return;
    };

    if !connected_clients.0.contains(&src_address) {
        info!("Received data from a new client, adding it to ConnectedClients");
        connected_clients.0.push(src_address);

        // we dont need to send the first bytes from a new client to other clients, currently thats just
        // a mock message so we can register the new client
        return;
    }

    if bytes.is_empty() {
        return;
    }

    if bytes.starts_with(&[REQUEST_NEW_NET_ENTITY_BYTE_HEADER]) {
        let temporary_net_id = bytes[1];
        // a client is requesting a new net entity
        info!(
            "Client {src_address:?} is requesting new net entity for temporary net id: {temporary_net_id}"
        );

        let net_entity_id = next_net_entity_id.0;

        commands.spawn(NetEntityId(net_entity_id));

        let res = current_server_socket.0.0.send_to(
            &[
                CONFIRM_NEW_NET_ENTITY_BYTE_HEADER,
                temporary_net_id,
                net_entity_id,
            ],
            src_address,
        );
        match res {
            Ok(_) => {
                info!("Sent confirm new net entity to client {}", src_address);
            }
            Err(error) => {
                // TODO: Should probably retry
                error!(
                    "Failed to sent confirm new net entity to client {}: {}",
                    src_address, error
                );
            }
        }
        next_net_entity_id.0 += 1;
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
                debug!("Sent {} bytes to {}", count_b, connected_client);
                debug!("Bytes sent are: {:?}", bytes);
            }
            Err(error) => {
                error!("Couldnt sent bytes: {}", error);
            }
        }
    }
}

pub fn handle_start_server(event: On<StartServer>, mut commands: Commands) {
    info!("Handling StartServer event");
    let socket = bind_socket(event.port);
    commands.insert_resource(CurrentServerSocket(socket));
}
