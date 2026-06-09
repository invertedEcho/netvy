use std::net::{SocketAddr, UdpSocket};

use bevy::{platform::collections::HashMap, prelude::*};

use crate::{
    CurrentSocket, PeerId, ReplicateEntity, TargetAddress,
    client::Client,
    net_entity::{NetEntity, NetEntityType},
    network_messages::{NetMessageId, NetworkMessageRegistry},
    prelude::MessageDirection,
    util::{
        DatagramType, bind_socket, get_byte_header_for_datagram_type, get_datagram_type,
        parse_u32_from_u8_arr, receive_all_packets_from_socket,
    },
};

pub mod prelude {
    pub use crate::server::{Server, StartServer};
}

/// Stores the next available net entity id. Only the server knows this and has authority about this.
#[derive(Resource, Default)]
pub struct NextNetEntityId(pub u8);

/// Stores all connected clients so we know to which address to send data to
#[derive(Resource, Default, Debug)]
pub struct ConnectedClients(pub Vec<SocketAddr>);

/// Trigger this Event to start a local server
#[derive(Event)]
pub struct StartServer {
    pub server_entity: Entity,
}

/// Stores the next available peer_id. Only the server is allowed to generate these.
#[derive(Resource, Default)]
struct NextPeerId(pub u32);

/// A marker component for a server
#[derive(Component)]
pub struct Server;

pub struct NetvyServerPlugin;

impl Plugin for NetvyServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConnectedClients>()
            .init_resource::<NewClientsQueue>()
            .init_resource::<ClientRequestNewNetEntityIdQueue>()
            .init_resource::<ComponentUpdateQueue>()
            .init_resource::<SocketAddrToPeerId>()
            .init_resource::<NextPeerId>()
            .init_resource::<NetworkMessageQueue>();

        app.add_observer(handle_start_server);

        app.add_systems(
            Update,
            (
                handle_server_data,
                handle_component_update_queue,
                handle_new_clients_queue,
                handle_client_request_new_net_entity_queue,
                handle_network_message_queue,
                handle_new_replicate_entities_server,
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
    /// A temporary peer id, as only the server is allowed to create new ones, and this request
    /// comes from the client.
    temporary_peer_id: u32,
}

// TODO: we might want to expose this
#[derive(Resource, Default)]
struct SocketAddrToPeerId(pub HashMap<SocketAddr, PeerId>);

#[derive(Resource, Default)]
struct ClientRequestNewNetEntityIdQueue(Vec<ClientRequestNewNetEntityId>);

struct ClientRequestNewNetEntityId {
    src_address: SocketAddr,
    temporary_net_entity_id: u8,
}

#[derive(Resource, Default)]
struct ComponentUpdateQueue(Vec<ComponentUpdate>);

struct ComponentUpdate {
    src_address: SocketAddr,
    bytes: Vec<u8>,
}

#[derive(Resource, Default)]
struct NetworkMessageQueue(Vec<NetworkMessage>);

struct NetworkMessage {
    src_address: SocketAddr,
    bytes: Vec<u8>,
}

/// Receive all bytes from this current tick from the current server socket.
pub fn handle_server_data(world: &mut World) {
    let server_socket = world.resource::<CurrentSocket>();

    for (bytes, src_address) in receive_all_packets_from_socket(&server_socket.0) {
        let Some(datagram_type) = get_datagram_type(&bytes) else {
            return;
        };

        match datagram_type {
            DatagramType::NotifyInitialConnection => {
                let Ok(temporary_client_id) = parse_u32_from_u8_arr(&bytes, 1, 5) else {
                    warn!(
                        "Received a NotifyInitialConnection datagram without a temporary_client_id"
                    );
                    continue;
                };
                world.resource_mut::<NewClientsQueue>().0.push(NewClient {
                    src_address,
                    temporary_peer_id: temporary_client_id,
                });
            }
            DatagramType::ClientRequestNewNetEntity => {
                // a client is requesting a new net entity
                let temporary_net_entity_id = bytes[1];

                world
                    .resource_mut::<ClientRequestNewNetEntityIdQueue>()
                    .0
                    .push(ClientRequestNewNetEntityId {
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
            DatagramType::NetworkMessage => {
                world
                    .resource_mut::<NetworkMessageQueue>()
                    .0
                    .push(NetworkMessage { bytes, src_address });
            }
            // The server doesnt receive these, it sends them to the client.
            DatagramType::ConfirmNetEntityRequest
            | DatagramType::SyncExistingNetEntities
            | DatagramType::AnnounceNewNetEntity
            | DatagramType::ConfirmClientConnect
            | DatagramType::AnnounceNewClient => {}
        }
    }
}

fn handle_new_clients_queue(
    mut commands: Commands,
    mut new_clients_queue: ResMut<NewClientsQueue>,
    mut connected_clients: ResMut<ConnectedClients>,
    net_entities: Query<&NetEntity>,
    server_socket: Res<CurrentSocket>,
    mut next_peer_id: ResMut<NextPeerId>,
    mut socket_addr_to_peer_id: ResMut<SocketAddrToPeerId>,
) {
    for NewClient {
        src_address,
        temporary_peer_id,
    } in new_clients_queue.0.drain(0..)
    {
        info!("Received NewClient datagram");

        socket_addr_to_peer_id
            .0
            .insert(src_address, PeerId(next_peer_id.0));

        commands.spawn((Client, PeerId(next_peer_id.0)));

        send_confirm_client_connect(
            &server_socket.0,
            src_address,
            temporary_peer_id,
            next_peer_id.0,
        );

        let net_entities = net_entities.iter().map(|n| n.0).collect();
        sync_existing_net_entities(&server_socket.0, net_entities, src_address);

        // announce this new client to any connected clients
        for client in &connected_clients.0 {
            let mut data = Vec::new();
            data.push(get_byte_header_for_datagram_type(
                DatagramType::AnnounceNewClient,
            ));

            data.extend_from_slice(&next_peer_id.0.to_be_bytes());

            let res = server_socket.0.send_to(&data, client);
            debug!("{res:?}");
        }

        if !connected_clients.0.contains(&src_address) {
            connected_clients.0.push(src_address);
        }

        next_peer_id.0 += 1;
    }
}

fn send_confirm_client_connect(
    socket: &UdpSocket,
    client_address: SocketAddr,
    temporary_client_id: u32,
    client_id: u32,
) {
    let mut data = Vec::new();

    let byte_header = get_byte_header_for_datagram_type(DatagramType::ConfirmClientConnect);
    data.push(byte_header);

    data.extend_from_slice(&temporary_client_id.to_be_bytes());
    data.extend_from_slice(&client_id.to_be_bytes());

    let result = socket.send_to(&data, client_address);
    info!("ConfirmedClientConnect result: {result:?}");
}

// sync any existing net entities to the new client, so it can spawn entities for these
// net entities
fn sync_existing_net_entities(
    socket: &UdpSocket,
    net_entities: Vec<u8>,
    client_address: SocketAddr,
) {
    if net_entities.is_empty() {
        return;
    }

    let mut data = Vec::new();
    data.push(get_byte_header_for_datagram_type(
        DatagramType::SyncExistingNetEntities,
    ));

    data.extend_from_slice(&net_entities);

    let res = socket.send_to(&data, client_address);
    match res {
        Ok(_) => {
            info!("Notified {client_address} about existing net entities. Data sent: {data:?}");
        }
        Err(error) => {
            error!(
                "Failed to notify {client_address} about existing net entities. {}",
                error
            );
        }
    }
}

fn handle_client_request_new_net_entity_queue(
    mut commands: Commands,
    mut queue: ResMut<ClientRequestNewNetEntityIdQueue>,
    mut next_net_entity_id: ResMut<NextNetEntityId>,
    server_socket: Res<CurrentSocket>,
    connected_clients: Res<ConnectedClients>,
) {
    for ClientRequestNewNetEntityId {
        src_address,
        temporary_net_entity_id,
    } in queue.0.drain(0..)
    {
        info!(
            "Client {src_address:?} is requesting new net entity id for temporary net id: {temporary_net_entity_id}"
        );

        let net_entity_id = next_net_entity_id.0;

        commands.spawn((NetEntity(net_entity_id), NetEntityType::Remote));

        let res = server_socket.0.send_to(
            &[
                get_byte_header_for_datagram_type(DatagramType::ConfirmNetEntityRequest),
                temporary_net_entity_id,
                net_entity_id,
            ],
            src_address,
        );
        match res {
            Ok(_) => {
                debug!(
                    "Sent confirm new net entity to client {} (net_entity_id={net_entity_id}, temporary_net_entity_id={temporary_net_entity_id})",
                    src_address
                );
            }
            Err(error) => {
                // TODO: Should probably retry
                error!(
                    "Failed to sent net entity confirmation (client={}): {}",
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
                        "Failed to announce new net entity to client (client={connected_client}, net_entity_id={net_entity_id}): {error}"
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

pub fn handle_start_server(
    event: On<StartServer>,
    mut commands: Commands,
    server_query: Query<&TargetAddress, With<Server>>,
) {
    let Ok(target_address) = server_query.get(event.server_entity) else {
        error!(
            "Failed to find TargetAddress for given server_entity. Either your server does not have the required TargetAddress component or the given entity does not exist."
        );
        return;
    };

    let socket = bind_socket(target_address.port);
    commands.insert_resource(CurrentSocket(socket));
    info!(
        "Started server on address {} and port {}",
        target_address.address, target_address.port
    );
}

fn handle_network_message_queue(world: &mut World) {
    let messages = {
        let mut queue = world.resource_mut::<NetworkMessageQueue>();
        std::mem::take(&mut queue.0)
    };

    for network_message in messages {
        let bytes = network_message.bytes;

        match parse_u32_from_u8_arr(&bytes, 1, 5) {
            Ok(network_message_id) => {
                let network_message_id = NetMessageId(network_message_id);

                let message_entry = {
                    world
                        .resource::<NetworkMessageRegistry>()
                        .message_entry
                        .get(&network_message_id)
                        .copied()
                };

                let Some(message_entry) = message_entry else {
                    warn!(
                        "Failed to find message_entry for network_message_id {network_message_id:?}"
                    );
                    continue;
                };

                let message_direction = message_entry.direction;
                let net_message_handler = message_entry.net_message_handler;

                match message_direction {
                    MessageDirection::ClientToServer => {
                        let Some(peer_id) = world
                            .resource::<SocketAddrToPeerId>()
                            .0
                            .get(&network_message.src_address)
                            .copied()
                        else {
                            warn!(
                                "Failed to find PeerId by src_address for net message (src_address={})",
                                network_message.src_address
                            );
                            continue;
                        };
                        let message_bytes = &bytes[5..];

                        net_message_handler(world, message_bytes, &peer_id.0);
                    }
                    MessageDirection::ClientToClients => {
                        // forward to all clients
                        for connected_client in &world.resource::<ConnectedClients>().0 {
                            match world
                                .resource::<CurrentSocket>()
                                .0
                                .send_to(&bytes, connected_client)
                            {
                                Ok(_) => {
                                    debug!(
                                        "Forwarded network message to client (client={connected_client:?})"
                                    );
                                }
                                Err(error) => {
                                    error!(
                                        "Failed to forward network message to client (client={connected_client}): {error}"
                                    );
                                }
                            }
                        }
                    }
                    MessageDirection::ServerToClient => {
                        info!("Received a ServerToClient network message on the server, ignoring.");
                    }
                    MessageDirection::ServerToClients => {
                        info!(
                            "Received a ServerToClients network message on the server, ignoring."
                        );
                    }
                }
            }
            Err(error) => {
                error!("Failed to decode incoming network message: {error:?}");
            }
        }
    }
}

pub fn handle_new_replicate_entities_server(
    mut commands: Commands,
    query: Query<Entity, Added<ReplicateEntity>>,
    mut next_net_entity_id: ResMut<NextNetEntityId>,
) {
    for added_entity in query {
        let net_entity_id = NetEntity(next_net_entity_id.0);
        debug!("ReplicateEntity was added on entity {added_entity}, inserting {net_entity_id:?}");
        commands.entity(added_entity).insert(net_entity_id);
        next_net_entity_id.0 += 1;
    }
}
