use bevy::prelude::*;

use crate::{
    CurrentSocket, PeerId, ReplicateEntity, TargetAddress, TemporaryClientId,
    component_updates::{ComponentUpdates, get_component_update_from_datagram},
    net_entity::{
        NetEntity, NetEntityType, NextTemporaryNetId, TemporaryNetId,
        handle_new_temporary_net_entities,
    },
    network::connect_to_server,
    network_messages::{NetMessageId, NetworkMessageRegistry},
    sync_position::apply_internal_sync_position,
    util::{
        DatagramType, get_byte_header_for_datagram_type, get_datagram_type,
        parse_connect_to_server, parse_u32_from_u8_arr, receive_all_packets_from_socket,
    },
};

pub mod prelude {
    pub use crate::client::{Client, ConnectToServer, OurPeerId};
}

#[derive(Component, Reflect)]
pub enum ConnectionState {
    None,
    Connecting,
    Connected,
}

#[derive(Component)]
pub struct Client;

/// Trigger this event on the client to connect to a server
#[derive(Event)]
pub struct ConnectToServer {
    pub client_entity: Entity,
}

#[derive(Resource, Default)]
struct NextTemporaryClientId(pub u32);

/// Retrieve this resource to determine which client is yours, using the PeerId in this resource.
///
/// netvy automatically sets up this resource for you. Note that you may want to use Option<Res<>>,
/// as the resource may not exist yet if a client didn't yet connect to the server.
#[derive(Resource)]
pub struct OurPeerId(pub PeerId);

// TODO: rename client id here to peer id, because we will just use peer id everywhere.

/// Add this plugin on the client
pub struct NetvyClientPlugin;

impl Plugin for NetvyClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConfirmedNetEntityRequestsQueue>()
            .init_resource::<NextTemporaryClientId>();

        app.add_observer(handle_connect_trigger);

        app.add_systems(
            Update,
            (
                handle_data_client_socket,
                handle_new_temporary_net_entities,
                apply_internal_sync_position,
                handle_confirmed_net_entity_requests,
            ),
        );
    }
}

fn handle_connect_trigger(
    trigger: On<ConnectToServer>,
    mut commands: Commands,
    client_query: Query<(Entity, Option<&TargetAddress>), With<Client>>,
    mut next_temporary_client_id: ResMut<NextTemporaryClientId>,
) {
    let Ok((client_entity, target_address)) = client_query.get(trigger.event().client_entity)
    else {
        error!("Failed to ConnectToServer! The specified client_entity does not exist.");
        return;
    };

    let Some(target_address) = target_address else {
        error!(
            "Your specified client_entity doesn't have the required TargetAddress component present!"
        );
        return;
    };

    debug!("Handling ConnectToServer event");

    commands.entity(client_entity).insert((
        ConnectionState::Connecting,
        TemporaryClientId(next_temporary_client_id.0),
    ));

    let address = parse_connect_to_server(&target_address.address, target_address.port);

    let Some(client_socket) = connect_to_server(address) else {
        error!("Failed to connect to server at {address:?}");
        return;
    };

    let mut data = Vec::new();

    let byte_header = get_byte_header_for_datagram_type(DatagramType::NotifyInitialConnection);

    data.push(byte_header);
    data.extend_from_slice(&next_temporary_client_id.0.to_be_bytes());

    client_socket
        .send(&data)
        .expect("Can send new connect message to server");

    debug!("Sending new connect message to server! {:?}", data);

    commands.insert_resource(CurrentSocket(client_socket));

    next_temporary_client_id.0 += 1;
}

struct ConfirmedNetEntityRequest {
    temporary_net_id: u8,
    net_entity_id: NetEntity,
}

#[derive(Resource, Default)]
struct ConfirmedNetEntityRequestsQueue(pub Vec<ConfirmedNetEntityRequest>);

fn handle_data_client_socket(world: &mut World) {
    let client_socket = world.resource::<CurrentSocket>();

    for (bytes, _) in receive_all_packets_from_socket(&client_socket.0) {
        let Some(datagram_type) = get_datagram_type(&bytes) else {
            return;
        };

        match datagram_type {
            DatagramType::ConfirmNetEntityRequest => {
                if bytes.len() < 2 {
                    error!(
                        "Received a ConfirmNewNetEntity message without entity net id, datagram: {bytes:?}"
                    );
                    return;
                }

                let temporary_net_id = bytes[1];
                let net_entity_id = bytes[2];

                let confirmed = ConfirmedNetEntityRequest {
                    temporary_net_id,
                    net_entity_id: NetEntity(net_entity_id),
                };
                world
                    .resource_mut::<ConfirmedNetEntityRequestsQueue>()
                    .0
                    .push(confirmed);
            }
            DatagramType::SyncExistingNetEntities => {
                let net_entities = &bytes[1..];

                for net_entity in net_entities {
                    // TODO: Im only 99% sure that only other entities will be included in the
                    // IncomingNewNetEntity message. Very unlikely but still...
                    let id = world
                        .spawn((NetEntity(*net_entity), NetEntityType::Remote))
                        .id();
                    info!(
                        "Spawned Entity {id} for SyncExistingNetEntities with net_entity_id: {net_entity}"
                    )
                }
            }
            DatagramType::ComponentUpdate => {
                let Some(component_update) = get_component_update_from_datagram(&bytes) else {
                    return;
                };
                let mut component_updates = world.resource_mut::<ComponentUpdates>();
                component_updates.0.push(component_update);
            }
            DatagramType::AnnounceNewNetEntity => {
                let new_net_entity = NetEntity(bytes[1]);

                info!("Received AnnounceNewNetEntity. Spawning new entity for {new_net_entity:?}");

                world.spawn((new_net_entity, NetEntityType::Remote));
            }
            DatagramType::ConfirmClientConnect => {
                let Ok(temporary_client_id) = parse_u32_from_u8_arr(&bytes, 1, 5) else {
                    error!(
                        "Failed to parse temporary_client_id from ConfirmClientConnect datagram"
                    );
                    continue;
                };
                let Ok(peer_id) = parse_u32_from_u8_arr(&bytes, 5, 9) else {
                    error!("Failed to parse peer_id from ConfirmClientConnect datagram");
                    continue;
                };

                let mut query = world.query::<(Entity, &TemporaryClientId)>();
                let Some((entity, _)) = query
                    .iter(world)
                    .find(|(_, temp)| temp.0 == temporary_client_id)
                else {
                    error!(
                        "Failed to find entity with temporary_client_id from ConfirmClientConnect datagram"
                    );
                    continue;
                };
                world
                    .entity_mut(entity)
                    .insert((ConnectionState::Connected, PeerId(peer_id)))
                    .remove::<TemporaryClientId>();

                world.insert_resource(OurPeerId(PeerId(peer_id)));

                info!(
                    "ConfirmClientConnect confirmed, updated local entity and inserted OurPeerId resource!"
                );
            }
            DatagramType::NetworkMessage => match parse_u32_from_u8_arr(&bytes, 1, 5) {
                Ok(net_message_id) => {
                    let message_entry = {
                        world
                            .resource::<NetworkMessageRegistry>()
                            .message_entry
                            .get(&NetMessageId(net_message_id))
                            .copied()
                    };

                    let Some(message_entry) = message_entry else {
                        error!(
                            "Failed to find message_entry for incoming network message id {net_message_id:?} in registry"
                        );
                        return;
                    };

                    let net_message_handler = message_entry.net_message_handler;
                    let message_bytes = &bytes[5..];
                    net_message_handler(world, message_bytes, &net_message_id);
                }
                Err(error) => {
                    error!("Failed to decode incoming network message: {error:?}");
                }
            },
            DatagramType::AnnounceNewClient => {
                let Ok(client_id) = parse_u32_from_u8_arr(&bytes, 1, 5) else {
                    error!("Could not parse client_id from AnnounceNewClient");
                    continue;
                };

                world.spawn((Client, PeerId(client_id)));
            }
            // A client doesnt receive these.
            DatagramType::ClientRequestNewNetEntity | DatagramType::NotifyInitialConnection => {}
        }
    }
}

fn handle_confirmed_net_entity_requests(
    mut commands: Commands,
    mut resource: ResMut<ConfirmedNetEntityRequestsQueue>,
    query: Query<(Entity, Option<&TemporaryNetId>, Option<&NetEntity>)>,
) {
    for ConfirmedNetEntityRequest {
        temporary_net_id: datagram_temp_id,
        net_entity_id,
    } in resource.0.drain(0..)
    {
        let Some(entity) = query
            .iter()
            .find(|(_, temporary_net_id, _)| {
                let Some(temporary_net_id) = temporary_net_id else {
                    return false;
                };
                temporary_net_id.0 == datagram_temp_id
            })
            .map(|(entity, _, _)| entity)
        else {
            error!(
                "Received a CONFIRM_NEW_NET_ENTITY message from server but couldnt find any entity that matches the temporary net id from datagram: {}",
                datagram_temp_id
            );
            return;
        };

        let mut entity_commands = commands.entity(entity);

        entity_commands.insert(net_entity_id);
        entity_commands.remove::<TemporaryNetId>();

        info!("Added confirmed {net_entity_id:?} from server into local entity {entity}");
    }
}

pub fn handle_new_sync_entities(
    mut commands: Commands,
    query: Query<Entity, Added<ReplicateEntity>>,
    mut next_temporary_net_entity_id: ResMut<NextTemporaryNetId>,
) {
    for added_entity in query {
        info!("SyncEntity was added on entity {added_entity}, adding TemporaryNetId");
        commands
            .entity(added_entity)
            .insert(TemporaryNetId(next_temporary_net_entity_id.0));
        next_temporary_net_entity_id.0 += 1;
    }
}
