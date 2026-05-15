use crate::{
    ComponentUpdate,
    net_entity::NetEntity,
    util::{DatagramType, get_byte_header_for_datagram_type},
};
use bevy::prelude::*;

pub fn build_component_update_datagram(
    component_bytes: &[u8],
    component_type_id: u8,
    net_entity_id: &NetEntity,
    current_update_sequence: u32,
) -> Vec<u8> {
    let mut data = Vec::new();

    data.extend_from_slice(&[get_byte_header_for_datagram_type(
        DatagramType::ComponentUpdate,
    )]);

    data.extend_from_slice(&[net_entity_id.0]);

    data.extend_from_slice(&[component_type_id]);

    let new_update_sequence = current_update_sequence.to_be_bytes();

    data.extend_from_slice(&new_update_sequence);

    data.extend_from_slice(component_bytes);
    data
}

pub fn get_component_update_from_datagram(bytes: &[u8]) -> Option<ComponentUpdate> {
    if bytes[0] != get_byte_header_for_datagram_type(DatagramType::ComponentUpdate) {
        return None;
    }

    // if bytes.len() < 8 {
    //     warn!("bytes are too short to be a ComponentUpdate. {bytes:?}");
    //     return None;
    // }

    match <[u8; 4]>::try_from(&bytes[3..7]) {
        Ok(result) => Some(ComponentUpdate {
            net_entity_id: NetEntity(bytes[1]),
            component_type_id: bytes[2],
            update_sequence: u32::from_be_bytes(result),
            component_bytes: bytes[7..].into(),
        }),
        Err(error) => {
            error!(
                "Failed to get sequence update bytes from component update datagram. bytes: {bytes:?}\n{error:?}"
            );
            None
        }
    }
}
