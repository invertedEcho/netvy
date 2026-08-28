use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;

use crate::utils::bind_socket_local;

/// Creates a UdpSocket and connects to the given server
/// This function does not ensure succesful connection
pub fn connect_to_server(server_address: SocketAddr) -> Option<UdpSocket> {
    let client_socket = bind_socket_local(0)?;

    info!(
        address = ?client_socket.local_addr(),
        "Succesfully binded UDP socket for client",
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
