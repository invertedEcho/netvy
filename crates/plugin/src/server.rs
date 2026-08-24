use std::net::{SocketAddr, UdpSocket};

use bevy::{platform::collections::HashMap, prelude::*};

use crate::{
    Authority, NetvyMode, OurPeerId, Owner, PeerId, ReplicateEntity, ServerSocket, TargetAddress,
    client::Client,
    component_updates::{
        ComponentUpdatesToBeApplied, LatestComponentUpdates, build_component_update_datagram,
        get_component_update_from_datagram,
    },
    net_entity::NetEntityId,
    network_messages::{MessageDirection, NetworkMessageId, NetworkMessageRegistry},
    util::{
        DatagramType, bind_socket_local, get_byte_header_for_datagram_type, get_datagram_type,
        parse_u32_from_u8_arr, receive_all_packets_from_socket,
    },
};

pub mod prelude {
    pub use crate::server::{Server, StartServer};
}

/// Stores the next available net entity id. Only the server knows this and has authority about this.
#[derive(Resource, Default)]
struct NextNetEntityId(pub u8);

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
            .init_resource::<ServerIncomingComponentUpdates>()
            .init_resource::<SocketAddrToPeerId>()
            .init_resource::<NextPeerId>()
            .init_resource::<NetworkMessageQueue>()
            .init_resource::<AnnounceNewNetEntityQueue>()
            .init_resource::<NextNetEntityId>();

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
                drain_announce_new_net_entity_queue,
            ),
        );
    }
}

/// A queue storing all new clients request, e.g. clients that are notifying the server they are
/// initially connecting
#[derive(Resource, Default)]
struct NewClientsQueue(pub Vec<NewClient>);

struct NewClient {
    client_address: SocketAddr,
    /// A temporary peer id, as only the server is allowed to create new ones, and this request
    /// comes from the client.
    temporary_peer_id: u32,
}

// TODO: we might want to expose this
#[derive(Resource, Default)]
pub struct SocketAddrToPeerId(pub HashMap<SocketAddr, PeerId>);

#[derive(Resource, Default)]
struct ClientRequestNewNetEntityIdQueue(Vec<ClientRequestNewNetEntityId>);

struct ClientRequestNewNetEntityId {
    src_address: SocketAddr,
    temporary_net_entity_id: u8,
}

/// Stores all component updates that the server received
#[derive(Resource, Default)]
struct ServerIncomingComponentUpdates(Vec<ServerComponentUpdate>);

struct ServerComponentUpdate {
    src_address: SocketAddr,
    bytes: Vec<u8>,
}

#[derive(Resource, Default)]
struct NetworkMessageQueue(Vec<NetworkMessage>);

struct NetworkMessage {
    src_address: SocketAddr,
    bytes: Vec<u8>,
}

#[derive(Resource, Default)]
struct AnnounceNewNetEntityQueue(Vec<AnnounceNewNetEntity>);

struct AnnounceNewNetEntity {
    net_entity: NetEntityId,
}

/// Receive all bytes from this current tick from the current server socket.
pub fn handle_server_data(world: &mut World) {
    let Some(server_socket) = world.get_resource::<ServerSocket>() else {
        trace!("No server socket exists, not handling any data");
        return;
    };

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

                debug!(
                    "A new client connected to our server, adding to NewClientsQueue (src_address={src_address}, temporary_peer_id={temporary_client_id})"
                );

                world.resource_mut::<NewClientsQueue>().0.push(NewClient {
                    client_address: src_address,
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
                world
                    .resource_mut::<ServerIncomingComponentUpdates>()
                    .0
                    .push(ServerComponentUpdate { bytes, src_address });
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
    net_entities: Query<&NetEntityId>,
    server_socket: If<Res<ServerSocket>>,
    mut next_peer_id: ResMut<NextPeerId>,
    mut socket_addr_to_peer_id: ResMut<SocketAddrToPeerId>,
    app_type: Res<NetvyMode>,
    latest_component_updates: ResMut<LatestComponentUpdates>,
) {
    for NewClient {
        client_address,
        temporary_peer_id,
    } in new_clients_queue.0.drain(0..)
    {
        for (key, value) in &latest_component_updates.0 {
            let component_type_id = key.1;

            let bytes =
                build_component_update_datagram(&value.0, component_type_id, &key.0, value.1);

            if let Err(error) = server_socket.0.0.send_to(&bytes, client_address) {
                error!(
                    "Failed to send latest component update to new client (error={error}, client_address={client_address}, component_type_id={component_type_id})"
                );
            } else {
                debug!(
                    ?client_address,
                    ?component_type_id,
                    "SNAPSHOT: Sent latest component update to new client"
                );
            }
        }
        let peer_id = PeerId(next_peer_id.0);

        socket_addr_to_peer_id.0.insert(client_address, peer_id);

        send_confirm_client_connect(
            &server_socket.0.0,
            client_address,
            temporary_peer_id,
            peer_id,
        );

        next_peer_id.0 += 1;

        // no need for everything below this check on HostClient, because server and client exist in
        // the same bevy world.
        if *app_type == NetvyMode::HostClient {
            return;
        }

        let client_entity = commands.spawn((Client, peer_id)).id();
        debug!(
            ?client_entity,
            ?client_address,
            ?temporary_peer_id,
            "Spawned a NewClient for item in NewClient queue"
        );

        let net_entities = net_entities.iter().map(|n| n.0).collect();
        sync_existing_net_entities(&server_socket.0.0, net_entities, client_address);

        // announce this new client to any connected clients
        for client in &connected_clients.0 {
            let mut data = Vec::new();
            data.push(get_byte_header_for_datagram_type(
                DatagramType::AnnounceNewClient,
            ));

            data.extend_from_slice(&peer_id.0.to_be_bytes());

            let result = server_socket.0.0.send_to(&data, client);
            debug!(
                "Announce new client {peer_id:?} to connected client {client:?}, result={result:?}"
            );
        }

        if !connected_clients.0.contains(&client_address) {
            connected_clients.0.push(client_address);
        }
    }
}

fn send_confirm_client_connect(
    socket: &UdpSocket,
    client_address: SocketAddr,
    temporary_peer_id: u32,
    peer_id: PeerId,
) {
    let byte_header = get_byte_header_for_datagram_type(DatagramType::ConfirmClientConnect);

    let mut data = Vec::new();

    data.push(byte_header);

    data.extend_from_slice(&temporary_peer_id.to_be_bytes());
    data.extend_from_slice(&peer_id.0.to_be_bytes());

    let result = socket.send_to(&data, client_address);

    if let Err(error) = result {
        error!("Failed to sent ConfirmClientConnect: {error}");
    }
}

// sync any existing net entities to the new client, so it can spawn entities for these
// net entities
fn sync_existing_net_entities(
    socket: &UdpSocket,
    net_entities: Vec<u8>,
    client_address: SocketAddr,
) {
    debug!(
        "Syncing {} net entities to {}",
        net_entities.len(),
        client_address
    );
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
            debug!(
                "Notified {client_address} about existing net entities: {:?}",
                net_entities
            );
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
    server_socket: If<Res<ServerSocket>>,
    connected_clients: Res<ConnectedClients>,
    socket_addr_to_peer_id: Res<SocketAddrToPeerId>,
    netvy_mode: Res<NetvyMode>,
) {
    for ClientRequestNewNetEntityId {
        src_address,
        temporary_net_entity_id,
    } in queue.0.drain(0..)
    {
        let Some(peer_id) = socket_addr_to_peer_id.0.get(&src_address) else {
            error!(
                "Cant handle new net entity request from client, origin address doesnt exist in SocketAddrToPeerId (src_address={src_address})"
            );
            continue;
        };

        let net_entity_id = next_net_entity_id.0;

        info!(
            client = ?src_address,
            ?temporary_net_entity_id,
            ?net_entity_id,
            "Assigning NetEntityId for requesting client and spawning this new NetEntity on the server: {}",
            *netvy_mode != NetvyMode::HostClient
        );

        // dont spawn otherwise we would end up with duplicate entity, because server is the same
        // bevy world because host client
        if *netvy_mode != NetvyMode::HostClient {
            commands.spawn((
                NetEntityId(net_entity_id),
                Owner(*peer_id),
                // TODO: bold assumption.. i think the user should decide this, but i guess providing a
                // sensible default cant hurt. but it could break things?
                // when a client requests spawning a new net entity, it will also get authority over
                // this entity.
                // I think we also would need to check if the client already had authority component
                // manually inserted, and if yes use that here instead.
                Authority(*peer_id),
            ));
        }

        let res = server_socket.0.0.send_to(
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

        for connected_client in &connected_clients.0 {
            // no need to announce the source client about itself
            if *connected_client == src_address {
                continue;
            }

            match server_socket.0.0.send_to(
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

        next_net_entity_id.0 += 1;
    }
}

fn handle_component_update_queue(
    mut queue: ResMut<ServerIncomingComponentUpdates>,
    connected_clients: Res<ConnectedClients>,
    server_socket: If<Res<ServerSocket>>,
    mut component_updates_to_be_applied: ResMut<ComponentUpdatesToBeApplied>,
) {
    for ServerComponentUpdate { bytes, src_address } in queue.0.drain(0..) {
        let Some(component_update) = get_component_update_from_datagram(&bytes) else {
            debug!(?bytes, "Invalid component update");
            continue;
        };

        component_updates_to_be_applied.0.push(component_update);
        for connected_client in &connected_clients.0 {
            // we of course dont need to send back the data we just received
            if *connected_client == src_address {
                continue;
            }

            let res = server_socket.0.0.send_to(&bytes, connected_client);
            match res {
                Ok(_) => {
                    trace!("Sent bytes {:?} to {}", bytes, connected_client);
                }
                Err(error) => {
                    error!(
                        "Couldnt sent ServerIncomingComponentUpdates bytes: {}",
                        error
                    );
                }
            }
        }
    }
}

fn handle_start_server(
    event: On<StartServer>,
    mut commands: Commands,
    server_query: Query<&TargetAddress, With<Server>>,
    mut next_peer_id: ResMut<NextPeerId>,
) {
    let Ok(target_address) = server_query.get(event.server_entity) else {
        error!(
            "Failed to find TargetAddress for given server_entity {}. Either your server does not have the required TargetAddress component or the given entity does not exist.",
            event.server_entity
        );
        return;
    };

    commands.insert_resource(OurPeerId(PeerId(next_peer_id.0)));

    commands
        .entity(event.server_entity)
        .insert(PeerId(next_peer_id.0));
    next_peer_id.0 += 1;

    let Some(socket) = bind_socket_local(target_address.0.port()) else {
        error!("Failed to start server!");
        return;
    };

    commands.insert_resource(ServerSocket(socket));

    info!("Started server on {:?}", target_address.0);
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
                let network_message_id = NetworkMessageId(network_message_id);

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
                let net_message_handler = message_entry.client_to_server_message_handler;

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

                        net_message_handler(world, message_bytes, peer_id);
                    }
                    MessageDirection::ClientToClients => {
                        // forward to all clients
                        for connected_client in &world.resource::<ConnectedClients>().0 {
                            match world
                                .resource::<ServerSocket>()
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

// We skip this system as long as we dont have a PeerId, e.g. server hasn't been created yet by the
// user. Using the Without<NetEntityId> filter we ensure that we dont visit previously handled net
// entities.
fn handle_new_replicate_entities_server(
    mut commands: Commands,
    query: Query<(Entity, Has<Authority>), (With<ReplicateEntity>, Without<NetEntityId>)>,
    mut next_net_entity_id: ResMut<NextNetEntityId>,
    mut announce_new_net_entity_queue: ResMut<AnnounceNewNetEntityQueue>,
    our_peer_id: If<Res<OurPeerId>>,
) {
    for (added_entity, has_authority) in query {
        let net_entity = NetEntityId(next_net_entity_id.0);
        debug!(
            ?net_entity,
            "ReplicateEntity was added server-side on entity {added_entity}, inserted NetEntityId and inserting Authority({:?}): {}",
            our_peer_id.0.0,
            !has_authority
        );

        // if a server spawns an entity, it automatically gets authority over this entity. but only
        // if the user didnt inserted authority previously, as we dont wanna override that.
        commands
            .entity(added_entity)
            .insert(net_entity)
            .insert_if(Authority(our_peer_id.0.0), || !has_authority);

        announce_new_net_entity_queue
            .0
            .push(AnnounceNewNetEntity { net_entity });

        next_net_entity_id.0 += 1;
    }
}

fn drain_announce_new_net_entity_queue(
    mut announce_new_net_entity_queue: ResMut<AnnounceNewNetEntityQueue>,
    connected_clients: Res<ConnectedClients>,
    server_socket: If<Res<ServerSocket>>,
) {
    for AnnounceNewNetEntity { net_entity } in announce_new_net_entity_queue.0.drain(0..) {
        for connected_client in &connected_clients.0 {
            let byte_header = get_byte_header_for_datagram_type(DatagramType::AnnounceNewNetEntity);

            let result = server_socket
                .0
                .0
                .send_to(&[byte_header, net_entity.0], connected_client);
            if let Err(error) = result {
                // TODO: add this to a failed queue, for this particular client
                error!(
                    "Failed to AnnounceNewNetEntity {net_entity:?} to client {connected_client}: {error}"
                );
            } else {
                debug!("Announced new net entity {net_entity:?} to client {connected_client}");
            }
        }
    }
}
