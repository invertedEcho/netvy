use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;

use crate::util::bind_socket;

/// Creates a UdpSocket and connects to the given server
/// This function does not ensure succesful connection
pub fn connect_to_server(server_address: SocketAddr) -> Option<UdpSocket> {
    let Some(client_socket) = bind_socket(0) else {
        return None;
    };

    info!(
        "Local udp socket for client binded {:?}",
        client_socket.local_addr()
    );

    let connect_result = client_socket.connect(server_address);

    match connect_result {
        Ok(res) => {
            debug!("Connect OK: {:?}", res);
            Some(client_socket)
        }
        Err(error) => {
            error!("Connect NOT OK: {:?}", error);
            None
        }
    }
}
