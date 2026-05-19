use std::net::UdpSocket;

use bevy::prelude::*;

pub mod prelude {
    pub use crate::messages::NetworkMessageReceiver;
    pub use crate::messages::NetworkMessageSender;
}

#[derive(Resource, Default)]
pub struct NetworkMessageReceiver {
    socket: Option<UdpSocket>,
}

impl NetworkMessageReceiver {
    /// Read/Drain all network messages
    fn receive(&mut self) {
        let Some(ref socket) = self.socket else {
            warn!("You need to connect to a server first, before being able to receive!");
            return;
        };
    }
}

#[derive(Resource, Default)]
pub struct NetworkMessageSender {
    pub socket: Option<UdpSocket>,
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
