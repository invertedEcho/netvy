use std::{
    io,
    net::{SocketAddr, UdpSocket},
};

use bevy::prelude::*;

pub fn receive_bytes_from_socket(socket: &UdpSocket) -> Option<(Vec<u8>, SocketAddr)> {
    let mut buf = [0; 1000];

    match socket.recv_from(&mut buf) {
        Ok((bytes_read, src_address)) => {
            if bytes_read == 0 {
                return None;
            }
            Some((buf[..bytes_read].to_vec(), src_address))
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => None,
        Err(e) => panic!("encountered IO error: {e}"),
    }
}

pub fn bind_socket(port: u16) -> UdpSocket {
    info!("Server started, socket binded on specified port {}", port);
    let server_socket =
        UdpSocket::bind(format!("127.0.0.1:{}", port)).expect("Couldnt bind to address");
    server_socket.set_nonblocking(true).unwrap();
    server_socket
}
