use std::net::UdpSocket;

use bevy::prelude::*;

#[derive(Resource)]
pub struct NetworkMessageReceiver {
    socket: Option<UdpSocket>,
}

impl NetworkMessageReceiver {
    /// Read/Drain all network messages
    fn receive(&mut self) {
        let Some(ref socket) = self.socket else {
            warn!("You need to connect to a server first before being able too send!");
            return;
        };
    }
}

#[derive(Resource)]
pub struct NetworkMessageSender {
    socket: Option<UdpSocket>,
}

impl NetworkMessageSender {
    /// Send a single network message
    fn send(&mut self, message: &[u8]) {
        let Some(ref socket) = self.socket else {
            warn!("You need to connect to a server first before being able too send!");
            return;
        };
        send_message(socket, message);
    }
}

fn send_message(socket: &UdpSocket, message: &[u8]) {
    socket.send(message);
}
