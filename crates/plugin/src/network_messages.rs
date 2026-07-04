use bevy::{platform::collections::HashMap, prelude::*};
use bincode::error::DecodeError;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    AppType, BINCODE_CONFIG, CurrentSocket, PeerId,
    server::{ConnectedClients, Server},
    util::{DatagramType, get_byte_header_for_datagram_type},
};

pub mod prelude {
    pub use crate::network_messages::{
        AppNetMessageExt, MessageDirection, NetMessageReader, NetMessageWriter,
    };
}

pub struct NetworkMessagePlugin;

#[derive(Component)]
pub struct NetMessageReader<M> {
    messages: Vec<M>,
}

impl<M> Default for NetMessageReader<M> {
    fn default() -> Self {
        Self { messages: vec![] }
    }
}

#[derive(Component)]
pub struct NetMessageWriter<M> {
    net_message_id: NetMessageId,
    messages_to_write: Vec<M>,
}

impl<M> NetMessageWriter<M> {
    /// Writes a network message. The network message will be sent to the targets configured by the
    /// MessageDirection.
    pub fn write(&mut self, message: M) {
        self.messages_to_write.push(message);
    }
}

impl<M> NetMessageReader<M> {
    pub fn read(&mut self) -> Vec<M> {
        self.messages.drain(0..).collect()
    }
}

impl Plugin for NetworkMessagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkMessageRegistry>()
            .init_resource::<NextNetMessageId>();
        app.add_systems(Update, add_net_message_reader_and_writer);
    }
}

#[derive(Copy, Clone, Debug)]
pub enum MessageDirection {
    ClientToServer,
    ClientToClients,
    ServerToClient,
    ServerToClients,
}

// 0: world, 1: network message bytes (e.g. content), 2: peer id from which we received the message
type NetworkFn = fn(&mut World, &[u8], &u32);

type InsertReaderAndWriter = fn(&mut EntityCommands, &NetMessageId);

#[derive(Copy, Clone)]
pub struct NetworkMessageEntry {
    pub direction: MessageDirection,
    /// 0: world, 1: network message bytes (e.g. content), 2: peer id from which we received the message
    pub net_message_handler: NetworkFn,
    pub insert_reader_and_writer: InsertReaderAndWriter,
}

#[derive(Resource, Default)]
pub struct NetworkMessageRegistry {
    pub message_entry: HashMap<NetMessageId, NetworkMessageEntry>,
}

pub trait AppNetMessageExt<'a> {
    /// Registers a new network message
    fn register_net_message<M: DeserializeOwned + Serialize + Sync + Send + 'a + 'static>(
        &mut self,
        message_direction: MessageDirection,
    );
}

impl<'a> AppNetMessageExt<'a> for App {
    fn register_net_message<M: DeserializeOwned + Serialize + Sync + Send + 'static>(
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

        let net_message_id = NetMessageId(next_net_message_id);

        let message_entry = NetworkMessageEntry {
            direction: message_direction,
            net_message_handler: |world, bytes, origin_peer_id| {
                let Ok((message, _size)): Result<(M, usize), DecodeError> =
                    bincode::serde::decode_from_slice(bytes, BINCODE_CONFIG)
                else {
                    return;
                };

                let mut message_reader = world.query::<(&mut NetMessageReader<M>, &PeerId)>();

                let Some((mut message_reader, _)) = message_reader
                    .iter_mut(world)
                    .find(|(_, peer_id)| peer_id.0 == *origin_peer_id)
                else {
                    error!(
                        "Could not add incoming net message to buffer, local message reader could not be found by peer id"
                    );
                    return;
                };
                message_reader.messages.push(message);
            },
            insert_reader_and_writer: |entity_commands, net_message_id| {
                entity_commands.insert((
                    NetMessageReader::<M>::default(),
                    NetMessageWriter::<M> {
                        net_message_id: *net_message_id,
                        messages_to_write: vec![],
                    },
                ));
            },
        };

        network_message_registry
            .message_entry
            .insert(net_message_id, message_entry);

        self.add_systems(Update, flush_net_messages::<M>);

        info!(
            "Registered a new NetworkMessage {} with direction {:?}",
            std::any::type_name::<M>(),
            message_direction
        )
    }
}

#[derive(Resource, Default)]
struct NextNetMessageId(NetMessageId);

/// Identifies a registered network message (the type, not the actual message)
/// Included in each datagram at bytes[1]
#[derive(Eq, PartialEq, Hash, Default, Copy, Clone, Debug)]
pub struct NetMessageId(pub u32);

fn add_net_message_reader_and_writer(
    mut commands: Commands,
    query: Query<Entity, Or<(Added<PeerId>, Added<Server>)>>,
    network_message_registry: Res<NetworkMessageRegistry>,
) {
    for added_client in query {
        for (net_message_id, net_message_entry) in &network_message_registry.message_entry {
            (net_message_entry.insert_reader_and_writer)(
                &mut commands.entity(added_client),
                net_message_id,
            );
        }
    }
}

fn flush_net_messages<M: Serialize + 'static + Send + Sync>(
    mut query: Query<&mut NetMessageWriter<M>>,
    network_message_registry: Res<NetworkMessageRegistry>,
    app_type: Res<AppType>,
    connected_clients: Option<Res<ConnectedClients>>,
    socket: Option<Res<CurrentSocket>>,
) {
    let Some(socket) = socket else {
        trace!("Not yet connected, skipping flushing net messages");
        return;
    };

    for mut writer in &mut query {
        let net_message_id = writer.net_message_id;

        for msg in writer.messages_to_write.drain(..) {
            let Some(message_entry) = network_message_registry.message_entry.get(&net_message_id)
            else {
                error!("Failed to find message_entry for network_message_id {net_message_id:?}");
                continue;
            };
            let message_direction = message_entry.direction;

            let mut datagram = Vec::new();

            datagram.push(get_byte_header_for_datagram_type(
                DatagramType::NetworkMessage,
            ));

            datagram.extend_from_slice(&net_message_id.0.to_be_bytes());

            let bytes = bincode::serde::encode_to_vec(msg, BINCODE_CONFIG).unwrap();

            datagram.extend_from_slice(&bytes);

            trace!(
                "handling intercepted network_message_id: {net_message_id:?}, {app_type:?}, {connected_clients:?}"
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
                // FIXME: implement
                AppType::ClientAndServer => {
                    error!("ClientAndServer not handled yet");
                }
            }
        }
    }
}
