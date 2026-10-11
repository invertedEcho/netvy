use bevy::prelude::*;

use crate::{
    component_updates::ComponentUpdate,
    datagram_type::{DatagramType, get_byte_header_for_datagram_type},
    net_entity::NetEntityId,
    utils::parse_u32_from_u8_arr,
};

pub fn get_component_update_from_datagram(bytes: &[u8]) -> Option<ComponentUpdate> {
    if bytes[0] != get_byte_header_for_datagram_type(DatagramType::ComponentUpdate) {
        return None;
    }

    // if bytes.len() < 8 {
    //     warn!("bytes are too short to be a ComponentUpdate. {bytes:?}");
    //     return None;
    // }

    match parse_u32_from_u8_arr(bytes, 3, 7) {
        Ok(update_sequence_number) => Some(ComponentUpdate {
            net_entity_id: NetEntityId(bytes[1]),
            component_type_id: bytes[2],
            update_sequence: update_sequence_number,
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

pub fn build_component_update_datagram(
    component_bytes: &[u8],
    component_type_id: u8,
    net_entity_id: &NetEntityId,
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
