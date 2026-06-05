use bevy::prelude::*;

use crate::{
    CurrentSocket,
    util::{DatagramType, get_byte_header_for_datagram_type},
};

// TODO: fix this wording
/// A NetEntity identifies an entity (that is replicated) across clients and servers.
/// You can use this to apply an operation on a target entity and find out on which entities on the
/// server (e.g. remote entities) you have to apply this operation too.
#[derive(Component, Eq, Hash, PartialEq, Clone, Debug, Reflect, Copy)]
pub struct NetEntity(pub u8);

#[derive(Component)]
pub struct TemporaryNetId(pub u8);

#[derive(Resource, Default)]
pub struct NextTemporaryNetId(pub u8);

#[derive(Component, Reflect, PartialEq)]
pub enum NetEntityType {
    Local,
    Remote,
}

// should only run on the client, because only clients have temporary net ids
pub fn handle_new_temporary_net_entities(
    query: Query<(Entity, &TemporaryNetId), Added<TemporaryNetId>>,
    current_socket: If<Res<CurrentSocket>>,
) {
    for (entity, temporary_net_id) in query {
        let result = current_socket.0.0.send(&[
            get_byte_header_for_datagram_type(DatagramType::ClientRequestNewNetEntity),
            temporary_net_id.0,
        ]);
        match result {
            Ok(_) => debug!(
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
