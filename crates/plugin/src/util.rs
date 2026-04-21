use std::net::SocketAddr;

use crate::ConnectToServer;

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
