use std::{
    io,
    net::{SocketAddr, UdpSocket},
};

use bevy::prelude::*;

pub fn connect_to_server(address: SocketAddr) -> UdpSocket {
    let client_socket = UdpSocket::bind("127.0.0.1:0").expect("Couldnt bind to address");
    client_socket.set_nonblocking(true).unwrap();
    info!(
        "Local udp socket for client binded {:?}",
        client_socket.local_addr()
    );

    let connect_result = client_socket.connect(address);
    match connect_result {
        Ok(res) => {
            debug!("Connect OK: {:?}", res);
        }
        Err(error) => {
            debug!("Connect NOT OK: {:?}", error);
        }
    }

    let send_result = client_socket.send(&[1]);
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
    let result = client_socket.recv(buf);
    match result {
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
            info!("Successfully connected to server at: {:?}", address);
        }
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
    client_socket
}
