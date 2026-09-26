use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    NetvyMode, OurPeerId, PeerId,
    component_updates::component_registry::AppComponentExt,
    net_entity::NetEntityId,
    network_messages::{AppNetworkMessageExt, FromServer, MessageDirection},
};

pub mod prelude {
    pub use crate::authority::{Authoritative, Authority};
}

/// This component is used to determine which peer has authority over the entity.
///
/// Authority means the ability to mutate state of an entity, e.g. its components
///
/// In order to avoid having to manually compare peer ids, you can filter by the `Authoritative` component,
/// to only get entities on which the current peer has authority over.
///
/// Note that the server can always mutate state of any entity, even if it doesn't have authority
/// over that entity.
/// If you have a valid use-case where you would not like this to happen, please open an issue in
/// the github repository.
#[derive(Component, Serialize, Deserialize, Debug, Reflect)]
pub struct Authority(pub PeerId);

/// You can filter by this component on any replicated entity to only get entities that the
/// current peer has authority over. Netvy automatically inserts this component for you.
#[derive(Component)]
pub struct Authoritative;

#[derive(Message, Serialize, Deserialize, Clone)]
pub struct UpdateAuthority {
    pub net_entity_id: NetEntityId,
    pub new_authority: PeerId,
}

pub struct AuthorityPlugin;

impl Plugin for AuthorityPlugin {
    fn build(&self, app: &mut App) {
        app.register_component::<Authority>();

        app.init_resource::<UpdateAuthorityQueue>();
        app.register_network_message::<UpdateAuthority>(MessageDirection::ServerToClients);

        app.add_systems(FixedUpdate, add_authoritative);

        app.add_systems(
            FixedUpdate,
            (read_update_authority, handle_update_authority_queue)
                .run_if(resource_equals(NetvyMode::Client)),
        );

        app.register_type::<Authority>();
    }
}

fn add_authoritative(
    mut commands: Commands,
    query: Query<(Entity, &Authority), Added<Authority>>,
    our_peer_id: Option<Res<OurPeerId>>,
) {
    for (entity, authority) in query {
        // NOTE: has to be in the for loop, so it only runs when Authority was added on any entity
        let Some(ref our_peer_id) = our_peer_id else {
            warn!(
                "Can't check if this entity should have Authoritative, OurPeerId resource doesn't exist yet. This entity miss the Authoritative component."
            );
            continue;
        };
        if authority.0 == our_peer_id.0 {
            commands.entity(entity).insert(Authoritative);
        }
    }
}

#[derive(Resource, Default)]
struct UpdateAuthorityQueue(Vec<UpdateAuthority>);

fn read_update_authority(
    mut message_reader: MessageReader<FromServer<UpdateAuthority>>,
    mut queue: ResMut<UpdateAuthorityQueue>,
) {
    // NOTE: we have the additional queue so we can use .retain()
    for message in message_reader.read() {
        debug!("Read UpdateAuthority message from server, adding to queue");
        queue.0.push(message.0.clone());
    }
}

fn handle_update_authority_queue(
    mut commands: Commands,
    mut queue: ResMut<UpdateAuthorityQueue>,
    query: Query<(Entity, &NetEntityId)>,
) {
    queue.0.retain(|item| {
        let UpdateAuthority {
            net_entity_id,
            new_authority,
        } = item;

        let Some(entity) = query.iter().find_map(|(entity, net_entity)| {
            if net_entity == net_entity_id {
                Some(entity)
            } else {
                None
            }
        }) else {
            debug!(
                ?net_entity_id,
                "Failed to handle UpdateAuthority net message: The given NetEntityId does not exist locally. Retrying again"
            );
            return true;
        };

        debug!(
            ?net_entity_id,
            ?new_authority,
            ?entity,
            "Updating authority for net entity"
        );
        commands.entity(entity).insert(Authority(*new_authority));
        false
    });
}
