use bevy::{platform::collections::HashMap, prelude::*};

use crate::{client::CurrentClientSocket, util::CLIENT_REQUEST_NEW_NET_ENTITY_BYTE_HEADER};

// we need to know to which entity to apply a change to across clients. bevy entity ids are not
// stable across worlds.
// so we need a network entity id.
// each client/server has a mapping for a given network entity id to its local entity id
#[derive(Resource, Default)]
pub struct NetEntityMapping(pub HashMap<NetEntityId, Entity>);

#[derive(Component, Eq, Hash, PartialEq, Clone, Debug, Reflect)]
pub struct NetEntityId(pub u8);

#[derive(Component)]
pub struct TemporaryNetId(pub u8);

#[derive(Resource, Default)]
pub struct NextTemporaryNetId(pub u8);

// TODO: Right now this works only semi-automatic
#[derive(Component)]
pub enum EntityType {
    Local,
    Remote,
}

// TODO:
// the net entity id must be the same across all clients, e.g.
// we must sync the NetEntityMapping across clients. if a client gets a new net entity, for what it
// doesnt have an entity yet, then we should spawn it
// then it can request a new net entity. the server will then respond hey you can use this net entity.
// then it can insert the net entity component

pub fn handle_new_temporary_net_entities(
    query: Query<(Entity, &TemporaryNetId), Added<TemporaryNetId>>,
    client_socket: Res<CurrentClientSocket>,
) {
    for (entity, temporary_net_id) in query {
        let result = client_socket.0.send(&[
            CLIENT_REQUEST_NEW_NET_ENTITY_BYTE_HEADER,
            temporary_net_id.0,
        ]);
        match result {
            Ok(_) => info!(
                "Send request for new net entity to server with TemporaryNetId: {:?}. Entity {}",
                temporary_net_id.0, entity
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
