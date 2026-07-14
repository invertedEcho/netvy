use std::any::TypeId;

use bevy::{platform::collections::HashMap, prelude::*};
use bincode::error::DecodeError;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    BINCODE_CONFIG, ClientSocket, NetvyMode, PeerId, ServerSocket,
    server::{ConnectedClients, SocketAddrToPeerId},
    util::{DatagramType, get_byte_header_for_datagram_type, reverse_hash_map_lookup},
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
            .init_resource::<NextNetworkMessageId>();
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
                send_client_to_server_messages::<M>.run_if(
                    resource_equals(NetvyMode::Client).or_else(resource_equals(NetvyMode::HostClient)),
                ),
                send_server_to_client_messages::<M>.run_if(
                    resource_equals(NetvyMode::Server).or_else(resource_equals(NetvyMode::HostClient)),
                ),
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
    client_socket: If<Res<ClientSocket>>,
    network_message_registry: Res<NetworkMessageRegistry>,
) {
    for message in message_reader.read() {
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

        if let Err(error) = client_socket.0.0.send(&datagram) {
            error!("Failed to forward local network message to the server: {error:?}");
        }
    }
}

fn send_server_to_client_messages<M: Message + Serialize>(
    mut message_reader: MessageReader<ToClients<M>>,
    server_socket: If<Res<ServerSocket>>,
    network_message_registry: Res<NetworkMessageRegistry>,
    connected_clients: Res<ConnectedClients>,
    socket_addr_to_peer_id: Res<SocketAddrToPeerId>,
) {
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
                    if let Err(error) = server_socket.0.0.send_to(&datagram, connected_client) {
                        error!(
                            "Failed to forward local network message to the client {connected_client}: {error:?}"
                        );
                    }
                }
            }
            NetworkMessageTarget::Clients(clients) => {
                for client_peer_id in clients {
                    let Some(socket_addr) =
                        reverse_hash_map_lookup(&socket_addr_to_peer_id.0, *client_peer_id)
                    else {
                        error!(
                            "Failed to forward network message to client {client_peer_id:?}, SocketAddrToPeerId doesnt contain this SocketAddr."
                        );
                        continue;
                    };

                    info!(
                        "Sending network message with target clients to client {client_peer_id:?}"
                    );
                    if let Err(error) = server_socket.0.0.send_to(&datagram, socket_addr) {
                        error!(
                            "Failed to forward local network message to the client {socket_addr} with peer_id {client_peer_id:?}: {error:?}"
                        );
                    }
                }
            }
        }
    }
}
