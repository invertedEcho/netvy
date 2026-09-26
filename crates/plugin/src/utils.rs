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

pub fn filter_component_log(component_type_id: u8) -> bool {
    let mut env_vars = std::env::vars();
    let Some(component_update_filter) = env_vars.find(|(key, _)| key == "FILTER_COMPONENT_TYPE_ID")
    else {
        return true;
    };
    let Ok::<u8, _>(parsed) = component_update_filter.1.parse() else {
        warn!("FILTER_COMPONENT_TYPE_ID couldnt be parsed, make sure you are passing a valid u8");
        return true;
    };

    parsed == component_type_id
}
