use std::any::TypeId;

use bevy::{platform::collections::HashMap, prelude::*};
use bincode::error::DecodeError;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    BINCODE_CONFIG, ClientSocket, NetvyMode, PeerId, ServerSocket,
    server::{ConnectedClients, SocketAddrToPeerId},
    util::{DatagramType, get_byte_header_for_datagram_type},
};

pub mod prelude {
    pub use crate::network_messages::{
        AppNetworkMessageExt, FromClient, FromServer, MessageDirection, NetworkMessageTarget,
        ToClients, ToServer,
    };
}

pub struct NetworkMessagePlugin;

// For sending network message from a client to the server. Server can know from which peer id this
// came from.
#[derive(Message)]
pub struct FromClient<M> {
    pub message: M,
    pub source_client: PeerId,
}

#[derive(Serialize)]
pub enum NetworkMessageTarget {
    /// Sends the network message to all currently connected clients
    All,
    /// Sends the network message to the specified clients. If you want to send a network message to
    /// a single client only, use this with a Vec containing only the single client.
    Clients(Vec<PeerId>),
}

// For sending network message from server to specified clients target
#[derive(Message, Serialize)]
pub struct ToClients<M> {
    pub message: M,
    pub target: NetworkMessageTarget,
}

#[derive(Message, Serialize)]
pub struct ToServer<M: Message>(pub M);

#[derive(Message, Serialize)]
pub struct FromServer<M: Message>(pub M);

impl Plugin for NetworkMessagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkMessageRegistry>()
            .init_resource::<NextNetworkMessageId>()
            .init_resource::<HostClientNetworkMessages>();

        // app.add_systems(
        //     FixedUpdate,
        //     (handle_host_client_net_message_queue.run_if(resource_equals(NetvyMode::HostClient)),),
        // );
    }
}

#[derive(Copy, Clone, Debug)]
pub enum MessageDirection {
    ClientToServer,
    ClientToClients,
    ServerToClient,
    ServerToClients,
}

/// 0: world, 1: network message bytes (e.g. content), 2: from which client this message was sent from
type ClientToServerNetworkMessageHandler = fn(&mut World, &[u8], PeerId);

type ServerToClientMessageHandler = fn(&mut World, &[u8]);

#[derive(Copy, Clone)]
pub struct NetworkMessageEntry {
    pub direction: MessageDirection,
    pub client_to_server_message_handler: ClientToServerNetworkMessageHandler,
    pub server_to_client_message_handler: ServerToClientMessageHandler,
}

#[derive(Resource, Default)]
pub struct NetworkMessageRegistry {
    pub mapping: HashMap<TypeId, NetworkMessageId>,
    pub message_entry: HashMap<NetworkMessageId, NetworkMessageEntry>,
}

impl NetworkMessageRegistry {
    fn get_network_message_id<M: 'static>(&self) -> Option<NetworkMessageId> {
        let type_id = TypeId::of::<M>();
        self.mapping.get(&type_id).copied()
    }
}

struct HostClientNetworkMessage {
    net_message_bytes: Vec<u8>,
    net_message_handler: ClientToServerNetworkMessageHandler,
    target_peer_id: PeerId,
}

#[derive(Resource, Default)]
struct HostClientNetworkMessages(Vec<HostClientNetworkMessage>);

pub trait AppNetworkMessageExt<'a> {
    /// Registers a new network message
    fn register_network_message<M: Message + DeserializeOwned + Serialize>(
        &mut self,
        message_direction: MessageDirection,
    );
}

impl<'a> AppNetworkMessageExt<'a> for App {
    fn register_network_message<M: Message + DeserializeOwned + Serialize>(
        &mut self,
        message_direction: MessageDirection,
    ) {
        let world = self.world_mut();

        let next_net_message_id = {
            let mut next_net_message_id = world.resource_mut::<NextNetworkMessageId>();

            let id = next_net_message_id.0.0;

            next_net_message_id.0.0 += 1;

            id
        };

        let mut network_message_registry = world.resource_mut::<NetworkMessageRegistry>();

        let net_message_id = NetworkMessageId(next_net_message_id);

        network_message_registry
            .mapping
            .insert(std::any::TypeId::of::<M>(), net_message_id);

        let message_entry = NetworkMessageEntry {
            direction: message_direction,
            client_to_server_message_handler: |world, bytes, origin_peer_id| {
                let Ok((message, _size)): Result<(M, usize), DecodeError> =
                    bincode::serde::decode_from_slice(bytes, BINCODE_CONFIG)
                else {
                    return;
                };

                world.write_message(FromClient {
                    message,
                    source_client: origin_peer_id,
                });
            },
            server_to_client_message_handler: |world, bytes| {
                let Ok((message, _size)): Result<(M, usize), DecodeError> =
                    bincode::serde::decode_from_slice(bytes, BINCODE_CONFIG)
                else {
                    return;
                };

                world.write_message(FromServer(message));
            },
        };

        network_message_registry
            .message_entry
            .insert(net_message_id, message_entry);

        self.add_message::<FromServer<M>>();
        self.add_message::<ToServer<M>>();

        self.add_message::<FromClient<M>>();
        self.add_message::<ToClients<M>>();

        self.add_systems(
            Update,
            (
                send_client_to_server_messages::<M>.run_if(resource_equals(NetvyMode::Client)),
                send_server_to_client_messages::<M>.run_if(resource_equals(NetvyMode::Server)),
            ),
        );

        info!(
            "Registered a new NetworkMessage {} with direction {:?}",
            std::any::type_name::<M>(),
            message_direction
        )
    }
}

#[derive(Resource, Default)]
struct NextNetworkMessageId(NetworkMessageId);

/// Identifies a registered network message (the type, not the actual message)
/// Included in each datagram at bytes[1]
#[derive(Eq, PartialEq, Hash, Default, Copy, Clone, Debug, Serialize)]
pub struct NetworkMessageId(pub u32);

fn send_client_to_server_messages<M: Message + Serialize>(
    mut message_reader: MessageReader<ToServer<M>>,
    client_socket: Option<Res<ClientSocket>>,
    network_message_registry: Res<NetworkMessageRegistry>,
) {
    let Some(client_socket) = client_socket else {
        error!("Can't send network message from client to server, ClientSocket doesnt exist!");
        return;
    };

    for message in message_reader.read() {
        info!("Sending network message to server");

        let Some(network_message_id) = network_message_registry.get_network_message_id::<M>()
        else {
            error!(
                "Failed to forward local network message to the server, could not find NetworkMessageId for this message"
            );
            continue;
        };

        let mut datagram = Vec::new();

        datagram.push(get_byte_header_for_datagram_type(
            DatagramType::NetworkMessage,
        ));

        datagram.extend_from_slice(&network_message_id.0.to_be_bytes());

        let bytes = bincode::serde::encode_to_vec(message, BINCODE_CONFIG).unwrap();

        datagram.extend_from_slice(&bytes);

        if let Err(error) = client_socket.0.send(&datagram) {
            error!("Failed to forward local network message to the server: {error:?}");
        }
    }
}

fn send_server_to_client_messages<M: Message + Serialize>(
    mut message_reader: MessageReader<ToClients<M>>,
    server_socket: Option<Res<ServerSocket>>,
    network_message_registry: Res<NetworkMessageRegistry>,
    connected_clients: Res<ConnectedClients>,
    peer_id_to_socket_addr: Res<SocketAddrToPeerId>,
) {
    let Some(server_socket) = server_socket else {
        error!("Can't send network message from server to client, ServerSocket doesnt exist!");
        return;
    };

    for message in message_reader.read() {
        debug!("Sending ToClients<> network message to clients");

        let Some(network_message_id) = network_message_registry.get_network_message_id::<M>()
        else {
            error!(
                "Failed to forward local network message to the server, could not find NetworkMessageId for this message"
            );
            continue;
        };

        let mut datagram = Vec::new();

        datagram.push(get_byte_header_for_datagram_type(
            DatagramType::NetworkMessage,
        ));

        datagram.extend_from_slice(&network_message_id.0.to_be_bytes());

        let bytes = bincode::serde::encode_to_vec(message, BINCODE_CONFIG).unwrap();

        datagram.extend_from_slice(&bytes);

        match &message.target {
            NetworkMessageTarget::All => {
                for connected_client in &connected_clients.0 {
                    if let Err(error) = server_socket.0.send_to(&datagram, connected_client) {
                        error!(
                            "Failed to forward local network message to the client {connected_client}: {error:?}"
                        );
                    }
                }
            }
            NetworkMessageTarget::Clients(clients) => {
                for connected_client in &connected_clients.0 {
                    let Some(peer_id) = peer_id_to_socket_addr.0.get(connected_client) else {
                        error!(
                            "Failed to forward network message to client {connected_client}, SocketAddrToPeerId doesnt contain this SocketAddr."
                        );
                        continue;
                    };

                    if !clients.contains(peer_id) {
                        continue;
                    }

                    if let Err(error) = server_socket.0.send(&datagram) {
                        error!(
                            "Failed to forward local network message to the client {connected_client}: {error:?}"
                        );
                    }
                }
            }
        }
    }
}

// fn handle_host_client_net_message_queue(world: &mut World) {
//     let mut messages = {
//         let mut queue = world.resource_mut::<HostClientNetworkMessages>();
//         std::mem::take(&mut queue.0)
//     };
//
//     for item in messages.drain(0..) {
//         (item.net_message_handler)(world, &item.net_message_bytes, item.target_peer_id);
//     }
// }
