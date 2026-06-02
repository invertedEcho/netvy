use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use bevy::prelude::*;
use bincode::error::DecodeError;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    AppType, BINCODE_CONFIG, CurrentSocket,
    net_entity::NetEntity,
    server::ConnectedClients,
    util::{DatagramType, get_byte_header_for_datagram_type},
};

pub mod prelude {
    pub use crate::network_messages::{AppNetMessageExt, MessageDirection, NetworkMessageContext};
}

pub struct NetworkMessagePlugin;

impl Plugin for NetworkMessagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkMessageRegistry>()
            .init_resource::<NextNetMessageId>();
    }
}

#[derive(Copy, Clone, Debug)]
pub enum MessageDirection {
    ClientToServer,
    ClientToClients,
    ServerToClient,
    ServerToClients,
}

/// Provides additional context about a received network message, such as where this mesage
/// came from
#[derive(Message)]
pub struct NetworkMessageContext<M> {
    pub sender: Option<NetEntity>,
    pub message: M,
}

type NetworkFn = fn(&mut World, &[u8]);

#[derive(Copy, Clone)]
pub struct NetworkMessageEntry {
    pub direction: MessageDirection,
    pub handler: NetworkFn,
}

#[derive(Resource, Default)]
pub struct NetworkMessageRegistry {
    pub message_entry: HashMap<NetworkMessageId, NetworkMessageEntry>,
    type_id_to_network_message_id: HashMap<TypeId, NetworkMessageId>,
}

pub trait AppNetMessageExt {
    /// Registers a new network message
    fn register_net_message<M: DeserializeOwned + Message + Serialize>(
        &mut self,
        message_direction: MessageDirection,
    );
}

impl AppNetMessageExt for App {
    fn register_net_message<M: DeserializeOwned + Message + Serialize>(
        &mut self,
        message_direction: MessageDirection,
    ) {
        let world = self.world_mut();

        let next_net_message_id = {
            let mut next_net_message_id = world.resource_mut::<NextNetMessageId>();

            let id = next_net_message_id.0.0;

            next_net_message_id.0.0 += 1;

            id
        };

        let mut network_message_registry = world.resource_mut::<NetworkMessageRegistry>();

        let network_message_id = NetworkMessageId(next_net_message_id);

        network_message_registry
            .type_id_to_network_message_id
            .insert(TypeId::of::<M>(), network_message_id);

        let message_entry = NetworkMessageEntry {
            direction: message_direction,
            handler: |world, bytes| {
                let Ok((message, _size)): Result<(M, usize), DecodeError> =
                    bincode::serde::decode_from_slice(bytes, BINCODE_CONFIG)
                else {
                    return;
                };

                world.write_message(message);
            },
        };

        network_message_registry
            .message_entry
            .insert(network_message_id, message_entry);

        self.add_message::<NetworkMessageContext<M>>();

        self.add_systems(Update, add_message_reader::<M>);

        info!(
            "Registered a new NetworkMessage {} with direction {:?}",
            std::any::type_name::<M>(),
            message_direction
        )
    }
}

/// every time the user sends a bevy message, we 'intercept' it here
fn add_message_reader<C: Message + Serialize>(
    socket: Res<CurrentSocket>,
    mut message_reader: MessageReader<C>,
    network_message_registry: Res<NetworkMessageRegistry>,
    app_type: Res<AppType>,
    connected_clients: Option<Res<ConnectedClients>>,
) {
    for message in message_reader.read() {
        let type_id = message.type_id();

        let Some(network_message_id) = network_message_registry
            .type_id_to_network_message_id
            .get(&type_id)
        else {
            error!("Failed to get network message id from type id");
            continue;
        };

        let Some(message_entry) = network_message_registry
            .message_entry
            .get(network_message_id)
        else {
            error!("Failed to find message_entry for network_message_id {network_message_id:?}");
            continue;
        };
        let message_direction = message_entry.direction;

        let mut datagram = Vec::new();

        datagram.push(get_byte_header_for_datagram_type(
            DatagramType::NetworkMessage,
        ));

        datagram.extend_from_slice(&network_message_id.0.to_be_bytes());

        let bytes = bincode::serde::encode_to_vec(message, BINCODE_CONFIG).unwrap();

        datagram.extend_from_slice(&bytes);

        info!(
            "handling intercepted network_message_id: {network_message_id:?}, {app_type:?}, {connected_clients:?}"
        );

        match *app_type {
            AppType::Client => match message_direction {
                MessageDirection::ClientToServer | MessageDirection::ClientToClients => {
                    let result = socket.0.send(&datagram);
                    debug!("{result:?}");
                }
                // this message is meant for us, do nothing
                MessageDirection::ServerToClient | MessageDirection::ServerToClients => {}
            },
            AppType::Server => match message_direction {
                MessageDirection::ClientToClients
                | MessageDirection::ServerToClient
                | MessageDirection::ServerToClients => {
                    let Some(ref connected_clients) = connected_clients else {
                        warn!(
                            "cant send message to clients, connected_clients resource is not initialized"
                        );
                        continue;
                    };
                    for connected_client in &connected_clients.0 {
                        // if connected_client == src_address {
                        //     continue;
                        // };

                        debug!(
                            "Read a message from a registered net message, sending it to connected client {connected_client:?}"
                        );

                        let result = socket.0.send_to(&datagram, connected_client);
                        debug!("{result:?}");
                    }
                }
                MessageDirection::ClientToServer => {}
            },
        }
    }
}

#[derive(Resource, Default)]
struct NextNetMessageId(NetworkMessageId);

/// Identifies a registered network message (the type, not the actual message)
/// Included in each datagram at bytes[1]
#[derive(Eq, PartialEq, Hash, Default, Copy, Clone, Debug)]
pub struct NetworkMessageId(pub u32);
