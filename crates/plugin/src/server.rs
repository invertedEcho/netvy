use std::net::SocketAddr;

use bevy::prelude::*;

use crate::{
    CurrentSocket,
    net_entity::{NetEntity, NetEntityType},
    network_messages::{NetworkMessageId, NetworkMessageRegistry},
    util::{
        DatagramType, bind_socket, get_byte_header_for_datagram_type, get_datagram_type,
        parse_u32_from_u8_arr, receive_all_packets_from_socket,
    },
};

#[derive(Resource, Default)]
pub struct NextNetEntityId(pub u8);

/// Stores all connected clients so we know to which address to send data to
#[derive(Resource, Default)]
pub struct ConnectedClients(pub Vec<SocketAddr>);

/// Trigger this Event to start a local server
#[derive(Event)]
pub struct StartServer {
    /// The port on which the server should be started
    pub port: u16,
}

pub struct NetvyServerPlugin;

impl Plugin for NetvyServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConnectedClients>()
            .init_resource::<NewClientsQueue>()
            .init_resource::<ClientRequestNewNetEntityQueue>()
            .init_resource::<ComponentUpdateQueue>();

        app.add_observer(handle_start_server);

        app.add_systems(
            Update,
            (
                handle_server_data,
                handle_component_update_queue,
                handle_new_clients_queue,
                handle_client_request_new_net_entity_queue,
            ),
        );
    }
}

/// A queue storing all new clients request, e.g. clients that are notifying the server they are
/// initially connecting
#[derive(Resource, Default)]
struct NewClientsQueue(pub Vec<NewClient>);

struct NewClient {
    src_address: SocketAddr,
}

#[derive(Resource, Default)]
struct ClientRequestNewNetEntityQueue(Vec<ClientRequestNewNetEntity>);

struct ClientRequestNewNetEntity {
    src_address: SocketAddr,
    temporary_net_entity_id: u8,
}

#[derive(Resource, Default)]
struct ComponentUpdateQueue(Vec<ComponentUpdate>);

struct ComponentUpdate {
    src_address: SocketAddr,
    bytes: Vec<u8>,
}

/// Receive bytes from the current server socket.
/// The server will send relevant received bytes to all connected clients
pub fn handle_server_data(world: &mut World) {
    let server_socket = world.resource::<CurrentSocket>();

    for (bytes, src_address) in receive_all_packets_from_socket(&server_socket.0) {
        let Some(datagram_type) = get_datagram_type(&bytes) else {
            return;
        };

        match datagram_type {
            DatagramType::NewClient => {
                world
                    .resource_mut::<NewClientsQueue>()
                    .0
                    .push(NewClient { src_address });
            }
            DatagramType::ClientRequestNewNetEntity => {
                // a client is requesting a new net entity
                let temporary_net_entity_id = bytes[1];

                world
                    .resource_mut::<ClientRequestNewNetEntityQueue>()
                    .0
                    .push(ClientRequestNewNetEntity {
                        temporary_net_entity_id,
                        src_address,
                    });
            }
            DatagramType::ComponentUpdate => {
                debug!("Received ComponentUpdate datagram: {bytes:?}");
                world
                    .resource_mut::<ComponentUpdateQueue>()
                    .0
                    .push(ComponentUpdate { bytes, src_address });
            }
            DatagramType::NetworkMessage => match parse_u32_from_u8_arr(&bytes, 1, 5) {
                Ok(network_message_id) => {
                    let network_message_registry = world.resource::<NetworkMessageRegistry>();
                    let Some(func) = network_message_registry
                        .message
                        .get(&NetworkMessageId(network_message_id))
                    else {
                        error!(
                            "Failed to find fn for incoming network message id {network_message_id:?} in registry"
                        );
                        return;
                    };
                    let message_bytes = &bytes[5..];
                    func(world, message_bytes);
                }
                Err(error) => {
                    error!("Failed to decode incoming network message: {error:?}");
                }
            },
            // The server doesnt receive these, it sends them to the client.
            DatagramType::ConfirmNetEntityRequest
            | DatagramType::SyncExistingNetEntities
            | DatagramType::AnnounceNewNetEntity
            | DatagramType::ConfirmClientConnect => {}
        }
    }
}

fn handle_new_clients_queue(
    mut new_clients_queue: ResMut<NewClientsQueue>,
    mut connected_clients: ResMut<ConnectedClients>,
    net_entities: Query<&NetEntity>,
    server_socket: Res<CurrentSocket>,
) {
    for NewClient { src_address } in new_clients_queue.0.drain(0..) {
        info!("Received NewClient datagram");
        if connected_clients.0.contains(&src_address) {
            return;
        }

        // TODO: This new client must of course also be synced to any existing connected clients
        connected_clients.0.push(src_address);

        if net_entities.is_empty() {
            return;
        }

        // sync any existing (net) entities to new clients, so they can spawn entities for any existing
        // net entities
        let mut data = Vec::new();
        data.push(get_byte_header_for_datagram_type(
            DatagramType::SyncExistingNetEntities,
        ));

        let existing_net_entity_ids: Vec<u8> = net_entities.iter().map(|d| d.0).collect();
        data.extend_from_slice(&existing_net_entity_ids);

        let res = server_socket.0.send_to(&data, src_address);
        match res {
            Ok(_) => {
                info!("Notified {src_address} about existing net entities. Data sent: {data:?}");
            }
            Err(error) => {
                error!(
                    "Failed to notify {src_address} about existing net entities. {}",
                    error
                );
            }
        }

        let result = server_socket.0.send_to(
            &[get_byte_header_for_datagram_type(
                DatagramType::ConfirmClientConnect,
            )],
            src_address,
        );
        debug!("{result:?}");
    }
}

fn handle_client_request_new_net_entity_queue(
    mut commands: Commands,
    mut queue: ResMut<ClientRequestNewNetEntityQueue>,
    mut next_net_entity_id: ResMut<NextNetEntityId>,
    server_socket: Res<CurrentSocket>,
    connected_clients: Res<ConnectedClients>,
) {
    for ClientRequestNewNetEntity {
        src_address,
        temporary_net_entity_id: temporary_net_entity,
    } in queue.0.drain(0..)
    {
        info!(
            "Client {src_address:?} is requesting new net entity for temporary net id: {temporary_net_entity}"
        );

        let net_entity_id = next_net_entity_id.0;

        commands.spawn((NetEntity(net_entity_id), NetEntityType::Remote));

        let res = server_socket.0.send_to(
            &[
                get_byte_header_for_datagram_type(DatagramType::ConfirmNetEntityRequest),
                temporary_net_entity,
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

            match server_socket.0.send_to(
                &[
                    get_byte_header_for_datagram_type(DatagramType::AnnounceNewNetEntity),
                    net_entity_id,
                ],
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
}

fn handle_component_update_queue(
    mut queue: ResMut<ComponentUpdateQueue>,
    connected_clients: Res<ConnectedClients>,
    server_socket: Res<CurrentSocket>,
) {
    for ComponentUpdate { bytes, src_address } in queue.0.drain(0..) {
        for connected_client in &connected_clients.0 {
            // we of course dont need to send back the data we just received
            if *connected_client == src_address {
                continue;
            }

            let res = server_socket.0.send_to(&bytes, connected_client);
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
}

pub fn handle_start_server(event: On<StartServer>, mut commands: Commands) {
    debug!("Handling StartServer event");
    let socket = bind_socket(event.port);
    commands.insert_resource(CurrentSocket(socket));
}
