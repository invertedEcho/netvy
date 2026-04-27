use bevy::{platform::collections::HashMap, prelude::*};

use crate::client::CurrentClientSocket;

// TODO: uhh this is really bad, its too easy that a component update will start with these numbers
pub const REQUEST_NEW_NET_ENTITY_BYTE_HEADER: u8 = 255;
pub const CONFIRM_NEW_NET_ENTITY_BYTE_HEADER: u8 = 254;

// we need to know to which entity to apply a change to across clients. bevy entity ids are not
// stable across worlds.
// so we need a network entity id.
// each client/server has a mapping for a given network entity id to its local entity id
#[derive(Resource, Default)]
pub struct NetEntityMapping(pub HashMap<NetEntityId, Entity>);

#[derive(Component, Eq, Hash, PartialEq, Clone, Debug)]
pub struct NetEntityId(pub u8);

#[derive(Component)]
pub struct TemporaryNetId(pub u8);

#[derive(Resource, Default)]
pub struct NextTemporaryNetId(pub u8);

// TODO:
// the net entity id must be the same across all clients, e.g.
// we must sync the NetEntityMapping across clients. if a client gets a new net entity, for what it
// doesnt have an entity yet, then we should spawn it
// then it can request a new net entity. the server will then respond hey you can use this net entity.
// then it can insert the net entity component

pub fn handle_new_temporary_net_entities(
    query: Query<&TemporaryNetId, Added<TemporaryNetId>>,
    client_socket: Res<CurrentClientSocket>,
) {
    for new_entity in query {
        let result = client_socket
            .0
            .send(&[REQUEST_NEW_NET_ENTITY_BYTE_HEADER, new_entity.0]);
        match result {
            Ok(_) => info!(
                "Send request for new net entity to server with TemporaryNetId: {:?}",
                new_entity.0
            ),
            // TODO: In the case of an error we should of course retry
            Err(error) => error!(
                "Failed to send request for new net entity to server: {}",
                error
            ),
        }
    }
}

// TODO: I dont like this... it destroys the purpose/performance of a HashMap
pub fn get_net_entity_for_local_entity(
    mapping: &NetEntityMapping,
    local_entity: Entity,
) -> Option<&NetEntityId> {
    mapping
        .0
        .iter()
        .find(|(_, entity)| **entity == local_entity)
        .map(|(net_entity, _)| net_entity)
}
