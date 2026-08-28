use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    ClientSocket,
    utils::{DatagramType, get_byte_header_for_datagram_type},
};

/// A NetEntityId identifies an replicated entity across clients and servers.
#[derive(Component, Eq, Hash, PartialEq, Clone, Debug, Reflect, Copy, Serialize, Deserialize)]
pub struct NetEntityId(pub u8);

/// This component gets automatically inserted into entities that should be replicated, from a client.
/// netvy will then query for added entities with this component and send a request to the server.
/// The server will then validate the request and send back the actual NetEntityId along with this
/// TemporaryNetId. Right now, the server just hands out these NetEntityId's, but this makes it
/// possible to easily add rules in the future. Also, it ensures that two clients use the same net
/// entity id for different entities.
#[derive(Component, Debug)]
pub struct TemporaryNetId(pub u8);

#[derive(Resource, Default)]
pub struct NextTemporaryNetId(pub u8);

// should only run on the client, because only clients have temporary net ids, as servers can always
// create new net entity ids and spawn net entities.
pub fn handle_new_temporary_net_entities(
    query: Query<(Entity, &TemporaryNetId), Added<TemporaryNetId>>,
    client_socket: Option<Res<ClientSocket>>,
) {
    let Some(socket) = client_socket else {
        trace!("Skipping handle_new_temporary_net_entities, client socket doesnt exist yet");
        return;
    };

    for (entity, temporary_net_id) in query {
        let result = socket.0.send(&[
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
