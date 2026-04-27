use bevy::{platform::collections::HashMap, prelude::*};
use netvy_server::NextNetEntityId;
use shared::net_entity::REQUEST_NEW_NET_ENTITY_BYTE_HEADER_THINGY_THING;

use crate::{SyncEntity, client::CurrentClientSocket};

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
        let result = client_socket.0.send(&[
            REQUEST_NEW_NET_ENTITY_BYTE_HEADER_THINGY_THING,
            new_entity.0,
        ]);
        match result {
            Ok(_) => info!(
                "Send request for new net entity to server with {:?}",
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

// TODO: hmm maybe we can do it so we dont even need `SyncEntity` component? and we can just check
// entities with registered components and do it ourself? but this way i guess its more explicit for
// the users of this library
pub fn add_net_entity_id(
    mut commands: Commands,
    query: Query<Entity, Added<SyncEntity>>,
    mut net_entity_mapping: ResMut<NetEntityMapping>,
    next_net_entity_id: ResMut<NextNetEntityId>,
) {
    // we first have to ask the server hey does this net entity exist?
    // wait tha also doesnt work
    // i think only the server is allowed to say hey we have a new net entity, client please spawn a
    // local entity and save the local entity <-> net entity in the mapping
    for added_entity in query {
        let new_net_entity_id = NetEntityId(next_net_entity_id.0);

        commands
            .entity(added_entity)
            .insert(new_net_entity_id.clone());

        net_entity_mapping
            .0
            .insert(new_net_entity_id.clone(), added_entity);

        info!(
            "Added NetEntityId component into new synced entity! {new_net_entity_id:?} {added_entity:?}"
        );
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
