use std::net::SocketAddr;

use crate::client::ConnectToServer;

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

pub fn extract_component_type_id_from_btyes(bytes: &Vec<u8>) -> Option<u8> {
    Some(bytes[0])
}
