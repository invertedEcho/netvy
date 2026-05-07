use bevy::prelude::*;

use crate::{client::CurrentClientSocket, util::CLIENT_REQUEST_NEW_NET_ENTITY_BYTE_HEADER};

#[derive(Component, Eq, Hash, PartialEq, Clone, Debug, Reflect, Copy)]
pub struct NetEntityId(pub u8);

#[derive(Component)]
pub struct TemporaryNetId(pub u8);

#[derive(Resource, Default)]
pub struct NextTemporaryNetId(pub u8);

#[derive(Component, Reflect)]
pub enum NetEntityType {
    Local,
    Remote,
}

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
