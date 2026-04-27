use std::{
    io,
    net::{SocketAddr, UdpSocket},
};

use bevy::prelude::*;

use crate::{client::ConnectToServer, net_entity::NetEntityId};

pub fn parse_connect_to_server(event: &ConnectToServer) -> SocketAddr {
    SocketAddr::new(
        std::net::IpAddr::V4(
            event
                .server_url
                .parse()
                .expect("server_url must be a valid ipv4 address"),
        ),
        event.port,
    )
}

pub fn extract_component_type_id(bytes: &[u8]) -> Option<u8> {
    if bytes.is_empty() {
        None
    } else {
        Some(bytes[0])
    }
}

pub fn extract_net_entity_id(bytes: &[u8]) -> Option<NetEntityId> {
    if bytes.len() < 2 {
        None
    } else {
        Some(NetEntityId(bytes[1]))
    }
}

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
