use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;
use log::{debug, error, info};

pub fn connect_to_server(address: SocketAddr) {
    let socket = UdpSocket::bind("127.0.0.1:0").expect("Couldnt bind to address");
    info!(
        "Local udp socket for client binded {:?}",
        socket.local_addr()
    );

    let connect_result = socket.connect(&address);
    match connect_result {
        Ok(res) => {
            debug!("Connect OK: {:?}", res);
        }
        Err(error) => {
            debug!("Connect NOT OK: {:?}", error);
        }
    }

    let send_result = socket.send(&[1]);
    match send_result {
        Ok(res) => {
            debug!("Send OK: {:?}", res);
        }
        Err(error) => {
            debug!("Send NOT OK: {:?}", error);
        }
    }

    // recv once to see if server actually running
    let buf = &mut [];
    let result = socket.recv(buf);
    match result {
        Err(error) => {
            error!(
                "Could not connect to server. Please make sure that a server is running on the specified address: {}. {:?}",
                address, error
            );
        }
        Ok(_) => {
            info!("Successfully connected to server at: {:?}", address);
        }
    }
}

pub fn bind_server(port: u16) -> UdpSocket {
    info!("Server started, socket binded on specified port {}", port);
    let socket = UdpSocket::bind(format!("127.0.0.1:{}", port)).expect("Couldnt bind to address");
    socket.set_nonblocking(true).unwrap();
    socket
}
