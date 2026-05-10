use std::{
    io,
    net::{SocketAddr, UdpSocket},
};

use bevy::prelude::*;

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

pub fn receive_all_packets_from_socket(socket: &UdpSocket) -> Vec<(Vec<u8>, SocketAddr)> {
    let mut packets: Vec<(Vec<u8>, SocketAddr)> = Vec::new();
    let mut buf = [0; 1000];

    // its very important that we drain all packets each tick, so that no packets build up
    loop {
        match socket.recv_from(&mut buf) {
            Ok((bytes_read, src_address)) => {
                if bytes_read == 0 {
                    continue;
                }
                packets.push((buf[..bytes_read].to_vec(), src_address));
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => {
                error!("encountered IO error: {e}");
                continue;
            }
        };
    }

    packets
}

pub fn bind_socket(port: u16) -> UdpSocket {
    debug!("Binding socket on specified port {}", port);
    let socket = UdpSocket::bind(format!("127.0.0.1:{}", port)).expect("Couldnt bind to address");
    socket.set_nonblocking(true).unwrap();
    socket
}

#[derive(Debug)]
pub enum DatagramType {
    /// Sent when receiving a `ClientRequestNewNetEntity`, the server sends this to the client along
    /// with the new net entity id
    ConfirmNetEntityRequest,
    /// A server can sent this to a client alongside with any existing net entities, so that the
    /// client spawn local entities. This is used in cases like initial connection from client
    /// to server
    SyncExistingNetEntities,
    /// A client can send this to tell the server that a relevant entity changed locally. The server
    /// will send this to all other connected clients
    ComponentUpdate,
    /// A client can sent this to the server, whenever a client wants to spawn a new entity that
    /// should be synced across all connected clients. For that, the client first needs a NetEntityId
    ClientRequestNewNetEntity,
    /// A client can sent this to the server upon initial connection. Afterwards, `SyncExistingNetEntities` will be sent to that client
    NewClient,
    /// Server can send this to connected clients to announce a new net entity was created. Clients
    /// can then spawn a new entity for tihs new net entity
    AnnounceNewNetEntity,
}

pub fn get_datagram_type(bytes: &[u8]) -> Option<DatagramType> {
    if bytes.is_empty() {
        return None;
    }

    let first_byte = bytes[0];

    if first_byte == CONFIRM_NET_ENTITY_REQUEST_BYTE_HEADER {
        Some(DatagramType::ConfirmNetEntityRequest)
    } else if first_byte == SYNC_EXISTING_NET_ENTITIES_BYTE_HEADER {
        Some(DatagramType::SyncExistingNetEntities)
    } else if first_byte == COMPONENT_UPDATE_BYTE_HEADER {
        Some(DatagramType::ComponentUpdate)
    } else if first_byte == CLIENT_REQUEST_NEW_NET_ENTITY_BYTE_HEADER {
        Some(DatagramType::ClientRequestNewNetEntity)
    } else if first_byte == NEW_CLIENT_BYTE_HEADER {
        Some(DatagramType::NewClient)
    } else if first_byte == ANNOUNCE_NEW_NET_ENTITY_BYTE_HEADER {
        Some(DatagramType::AnnounceNewNetEntity)
    } else {
        debug!("Received invalid datagram: {bytes:?}");
        None
    }
}

pub const COMPONENT_UPDATE_BYTE_HEADER: u8 = 252;
pub const CLIENT_REQUEST_NEW_NET_ENTITY_BYTE_HEADER: u8 = 255;
pub const CONFIRM_NET_ENTITY_REQUEST_BYTE_HEADER: u8 = 254;

// For when a new client connects and that new client should spawn existing entities
// datagram:
// NEW_NET_ENTITY_BYTE_HEADER (u8) | existing_net_entities [u8]
pub const SYNC_EXISTING_NET_ENTITIES_BYTE_HEADER: u8 = 253;

/// A client can send this to the server to tell the server hey im a new client :wave_emoji: lol
pub const NEW_CLIENT_BYTE_HEADER: u8 = 250;
pub const ANNOUNCE_NEW_NET_ENTITY_BYTE_HEADER: u8 = 251;

// fn get_byte_header_for_datagram_type(datagram_type: DatagramType) -> u8 {
//     match datagram_type {
//         DatagramType::ComponentUpdate => COMPONENT_UPDATE_BYTE_HEADER,
//         DatagramType::ConfirmNewNetEntity => CONFIRM_NEW_NET_ENTITY_BYTE_HEADER,
//         DatagramType::IncomingNewNetEntity => NEW_NET_ENTITY_BYTE_HEADER,
//         DatagramType::ClientRequestNewNetEntity => CLIENT_REQUEST_NEW_NET_ENTITY_BYTE_HEADER,
//         DatagramType::NewClient => NEW_CLIENT_BYTE_HEADER,
//     }
// }
