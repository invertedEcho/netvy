use std::{
    io,
    net::{SocketAddr, UdpSocket},
};

use bevy::{platform::collections::HashMap, prelude::*};

pub fn reverse_hash_map_lookup<K, V>(hash_map: &HashMap<K, V>, value_to_search: V) -> Option<K>
where
    K: Copy,
    V: std::cmp::PartialEq<V>,
{
    hash_map
        .iter()
        .find(|(_key, value)| **value == value_to_search)
        .map(|(key, _value)| key)
        .copied()
}

pub fn parse_u32_from_u8_arr(bytes: &[u8], start: usize, end: usize) -> Result<u32> {
    let slice = &bytes[start..end];

    match <[u8; 4]>::try_from(slice) {
        Ok(result) => Ok(u32::from_be_bytes(result)),
        Err(error) => Err(error.into()),
    }
}

pub fn receive_all_packets_from_socket(socket: &UdpSocket) -> Vec<(Vec<u8>, SocketAddr)> {
    let mut packets: Vec<(Vec<u8>, SocketAddr)> = Vec::new();
    let mut buf = [0; 1000];

    // its very important that we drain all packets each tick, so that no packets build up
    loop {
        match socket.recv_from(&mut buf) {
            Ok((bytes_read, src_address)) => {
                if bytes_read == 0 {
                    break;
                }
                packets.push((buf[..bytes_read].to_vec(), src_address));
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => {
                error!("Failed to receive all current packets from socket {socket:?}: {e}");
                break;
            }
        };
    }

    packets
}

pub fn bind_socket_local(port: u16) -> Option<UdpSocket> {
    debug!("Binding socket on specified port {}", port);
    match UdpSocket::bind(format!("0.0.0.0:{}", port)) {
        Ok(socket) => {
            socket
                .set_nonblocking(true)
                .expect("Must be able to set the socket to be nonblocking");
            Some(socket)
        }
        Err(error) => {
            error!("Failed to bind socket on port {port}: {error:?}");
            None
        }
    }
}

// It may be wise to split this up into server and client datagrams, but some of them are
// sent/received on both client and server, so for now, we keep this
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
    /// A client can sent this to the server upon initial connection. This is also used to test connection to the server.
    /// Afterwards, `SyncExistingNetEntities` and `ConfirmClientConnect` datagram will be sent to that client
    NotifyInitialConnection,
    /// Server can send this to connected clients to announce a new net entity was created. Clients
    /// can then spawn a new entity for tihs new net entity. This is used right now when a client
    /// requests a new net entity id, then the server will send this message to all connected clients to notiify them.
    AnnounceNewNetEntity,
    /// A server can send this message to a client that sent a NotifyInitialConnection, to indicate it succesfully
    /// received the NotifyInitialConnection message. This is also used to test connection from client to server
    /// and vice versa
    ConfirmClientConnect,
    NetworkMessage,
    /// A server can send this message to notify any clients about a new client, so that these
    /// clients can spawn local clients.
    AnnounceNewClient,
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
    } else if first_byte == NOTIFY_INITIAL_CONNECTION_BYTE_HEADER {
        Some(DatagramType::NotifyInitialConnection)
    } else if first_byte == ANNOUNCE_NEW_NET_ENTITY_BYTE_HEADER {
        Some(DatagramType::AnnounceNewNetEntity)
    } else if first_byte == CONFIRM_CLIENT_CONNECT {
        Some(DatagramType::ConfirmClientConnect)
    } else if first_byte == NETWORK_MESSAGE_BYTE_HEADER {
        Some(DatagramType::NetworkMessage)
    } else if first_byte == ANNOUNCE_NEW_CLIENT_BYTE_HEADER {
        Some(DatagramType::AnnounceNewClient)
    } else {
        warn!("Received invalid datagram: {bytes:?}");
        None
    }
}

const CLIENT_REQUEST_NEW_NET_ENTITY_BYTE_HEADER: u8 = 255;
const CONFIRM_NET_ENTITY_REQUEST_BYTE_HEADER: u8 = 254;
const SYNC_EXISTING_NET_ENTITIES_BYTE_HEADER: u8 = 253;
const COMPONENT_UPDATE_BYTE_HEADER: u8 = 252;
const ANNOUNCE_NEW_NET_ENTITY_BYTE_HEADER: u8 = 251;
const NOTIFY_INITIAL_CONNECTION_BYTE_HEADER: u8 = 250;
const CONFIRM_CLIENT_CONNECT: u8 = 249;
const NETWORK_MESSAGE_BYTE_HEADER: u8 = 248;
const ANNOUNCE_NEW_CLIENT_BYTE_HEADER: u8 = 247;

pub fn get_byte_header_for_datagram_type(datagram_type: DatagramType) -> u8 {
    match datagram_type {
        DatagramType::ClientRequestNewNetEntity => CLIENT_REQUEST_NEW_NET_ENTITY_BYTE_HEADER,
        DatagramType::ConfirmNetEntityRequest => CONFIRM_NET_ENTITY_REQUEST_BYTE_HEADER,
        DatagramType::SyncExistingNetEntities => SYNC_EXISTING_NET_ENTITIES_BYTE_HEADER,
        DatagramType::ComponentUpdate => COMPONENT_UPDATE_BYTE_HEADER,
        DatagramType::AnnounceNewNetEntity => ANNOUNCE_NEW_NET_ENTITY_BYTE_HEADER,
        DatagramType::NotifyInitialConnection => NOTIFY_INITIAL_CONNECTION_BYTE_HEADER,
        DatagramType::ConfirmClientConnect => CONFIRM_CLIENT_CONNECT,
        DatagramType::NetworkMessage => NETWORK_MESSAGE_BYTE_HEADER,
        DatagramType::AnnounceNewClient => ANNOUNCE_NEW_CLIENT_BYTE_HEADER,
    }
}

pub fn should_log_component_update(component_type_id: u8) -> bool {
    let mut env_vars = std::env::vars();
    let Some(component_update_filter) = env_vars.find(|(key, _)| key == "FILTER_COMPONENT_TYPE_ID")
    else {
        return true;
    };
    let Ok::<u8, _>(parsed) = component_update_filter.1.parse() else {
        warn!("FILTER_COMPONENT_TYPE_ID couldnt be parsed, make sure you are passing a valid u8");
        return true;
    };

    return parsed == component_type_id;
}
