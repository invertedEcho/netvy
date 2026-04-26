use std::net::SocketAddr;

use crate::{NetEntityId, client::ConnectToServer};

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
