use std::net::UdpSocket;

use bevy::prelude::*;

pub mod prelude {
    pub use crate::network_messages::NetworkMessageReceiver;
    pub use crate::network_messages::NetworkMessageSender;
}

pub trait AppNetMessageExt {
    /// Registers a new network message
    fn register_net_message(&mut self) {}
}

impl AppNetMessageExt for App {
    fn register_net_message<C>(&mut self) {
        let world = self.world();
        let next_net_message_id = world.resource_mut::<NextNetMessageId>();
        world.spawn(NetworkMessageReceiver::<C> {
            id: next_net_message_id.0,
            messages: vec![],
        });
    }
}

#[derive(Resource)]
struct NextNetMessageId(NetworkMessageId);

struct NetworkMessageId(u64);

// somewhere we store an array of messages.
// we have a hashmap, and the key is the type of message, e.g. a MessageId
// a user calls register_message, and we create a NetworkMessageId for this new message
// this is an internal resource, here we store messages we read from a socket
// #[derive(Resource)]
// struct NetworkMessages(HashMap<NetworkMessageId, Vec>);
//
// doing it this way wont work. we need to do it like lightyear, at least for now,
// for each register_message, a new entity is created, with a NetworkMessageReceiver component. this
// component holds an array of messages of type C, which we can specify because we internally spawn
// tihs entity.

#[derive(Component)]
pub struct NetworkMessageReceiver<C>
where
    C: Clone,
{
    id: NetworkMessageId,
    messages: Vec<C>,
}

impl<C: Clone> NetworkMessageReceiver<C> {
    /// Read/Drain all network messages
    fn receive(&mut self) -> Vec<C> {
        self.messages.clone()
    }
}

impl NetworkMessageSender {
    /// Send a single network message
    fn send(&mut self, message: &[u8]) {
        let Some(ref socket) = self.socket else {
            warn!("You need to connect to a server first, before being able to send!");
            return;
        };
        send_message(socket, message);
    }
}

fn send_message(socket: &UdpSocket, message: &[u8]) {
    if let Err(error) = socket.send(message) {
        error!("Failed to send message: {error:?}");
    }
}
