use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;

use crate::{
    net_entity::NetEntityId,
    util::{
        ANNOUNCE_NEW_NET_ENTITY_BYTE_HEADER, CONFIRM_NET_ENTITY_REQUEST_BYTE_HEADER, DatagramType,
        SYNC_EXISTING_NET_ENTITIES_BYTE_HEADER, bind_socket, get_datagram_type,
        receive_bytes_from_socket,
    },
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
/// The server will send relevant received bytes to all connected clients
pub fn handle_server_data(
    mut commands: Commands,
    server_socket: If<Res<CurrentServerSocket>>,
    mut connected_clients: ResMut<ConnectedClients>,
    mut next_net_entity_id: ResMut<NextNetEntityId>,
    query: Query<&NetEntityId>,
) {
    let bytes = receive_bytes_from_socket(&server_socket.0.0);

    let Some((bytes, src_address)) = bytes else {
        return;
    };

    let Some(datagram_type) = get_datagram_type(&bytes) else {
        return;
    };

    match datagram_type {
        DatagramType::NewClient => {
            info!("Received NewClient datagram");
            if connected_clients.0.contains(&src_address) {
                return;
            }

            // TODO: This new client must of course also be synced to any existing connected clients
            connected_clients.0.push(src_address);

            // sync any existing (net) entities to new clients, so they can spawn entities for any existing
            // net entities
            let mut data = Vec::new();
            data.extend_from_slice(&[SYNC_EXISTING_NET_ENTITIES_BYTE_HEADER]);

            let existing_net_entities: Vec<u8> = query.iter().map(|d| d.0).collect();
            data.extend_from_slice(&existing_net_entities);

            let res = server_socket.0.0.send_to(&data, src_address);
            match res {
                Ok(_) => {
                    info!(
                        "Notified {src_address} about existing net entities. Data sent: {data:?}"
                    );
                }
                Err(error) => {
                    error!(
                        "Failed to notify {src_address} about existing net entities. {}",
                        error
                    );
                }
            }
        }
        DatagramType::ClientRequestNewNetEntity => {
            let temporary_net_id = bytes[1];
            // a client is requesting a new net entity
            info!(
                "Client {src_address:?} is requesting new net entity for temporary net id: {temporary_net_id}"
            );

            let net_entity_id = next_net_entity_id.0;

            commands.spawn(NetEntityId(net_entity_id));

            let res = server_socket.0.0.send_to(
                &[
                    CONFIRM_NET_ENTITY_REQUEST_BYTE_HEADER,
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

            for connected_client in &connected_clients.0 {
                // no need to announce the source client about itself
                if *connected_client == src_address {
                    continue;
                }

                match server_socket.0.0.send_to(
                    &[ANNOUNCE_NEW_NET_ENTITY_BYTE_HEADER, net_entity_id],
                    connected_client,
                ) {
                    Ok(_) => {
                        info!(
                            "Sent AnnounceNewNetEntity {net_entity_id:?} to client {connected_client}"
                        );
                    }
                    Err(error) => {
                        error!(
                            "Failed to send AnnounceNewNetEntity {net_entity_id:?} to client {connected_client}. {error}"
                        );
                    }
                }
            }
        }
        DatagramType::ComponentUpdate => {
            debug!("Received ComponentUpdate datagram: {bytes:?}");
            for connected_client in &connected_clients.0 {
                // we of course dont need to send back the data we just received
                if *connected_client == src_address {
                    continue;
                }

                let res = server_socket.0.0.send_to(&bytes, connected_client);
                match res {
                    Ok(_) => {
                        debug!("Sent bytes {:?} to {}", bytes, connected_client);
                    }
                    Err(error) => {
                        error!("Couldnt sent bytes: {}", error);
                    }
                }
            }
        }
        // The server doesnt receive these, it sends them to the client.
        DatagramType::ConfirmNetEntityRequest
        | DatagramType::SyncExistingNetEntities
        | DatagramType::AnnounceNewNetEntity => {}
    }
}

pub fn handle_start_server(event: On<StartServer>, mut commands: Commands) {
    debug!("Handling StartServer event");
    let socket = bind_socket(event.port);
    commands.insert_resource(CurrentServerSocket(socket));
}
