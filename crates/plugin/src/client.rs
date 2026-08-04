use bevy::prelude::*;

use crate::{
    ClientSocket, OurPeerId, PeerId, ReplicateEntity, TargetAddress, TemporaryPeerId,
    component_updates::{ComponentUpdatesToBeApplied, get_component_update_from_datagram},
    net_entity::{
        NetEntityId, NextTemporaryNetId, TemporaryNetId, handle_new_temporary_net_entities,
    },
    network::connect_to_server,
    network_messages::{NetworkMessageId, NetworkMessageRegistry},
    util::{
        DatagramType, get_byte_header_for_datagram_type, get_datagram_type, parse_u32_from_u8_arr,
        receive_all_packets_from_socket,
    },
};

pub mod prelude {
    pub use crate::client::{Client, ConnectToServer, ConnectionState};
}

/// The current connection state. Note that only the own client has this component
#[derive(Component, Reflect, PartialEq, Debug)]
pub enum ConnectionState {
    Connecting,
    Connected,
}

#[derive(Component)]
pub struct Client;

/// Trigger this event on a client entity to connect to a server.
/// This client entity must have the TargetAddress component added.
#[derive(Event)]
pub struct ConnectToServer {
    pub client_entity: Entity,
}

#[derive(Resource, Default)]
struct NextTemporaryPeerId(pub u32);

/// Add this plugin on the client
pub struct NetvyClientPlugin;

impl Plugin for NetvyClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConfirmedNetEntityRequestsQueue>()
            .init_resource::<NextTemporaryPeerId>();

        app.add_observer(handle_connect_trigger);

        app.add_systems(
            Update,
            (
                handle_data_client_socket.run_if(resource_exists::<ClientSocket>),
                handle_new_temporary_net_entities,
                handle_confirmed_net_entity_requests,
                handle_new_replicate_entities_client,
            ),
        );
    }
}

fn handle_connect_trigger(
    trigger: On<ConnectToServer>,
    mut commands: Commands,
    client_query: Query<(Entity, Option<&TargetAddress>), With<Client>>,
    mut next_temporary_peer_id: ResMut<NextTemporaryPeerId>,
) {
    let Ok((client_entity, target_address)) = client_query.get(trigger.event().client_entity)
    else {
        error!("Failed to ConnectToServer! The specified client_entity does not exist.");
        return;
    };

    let Some(target_address) = target_address else {
        error!(
            "Your specified client_entity {client_entity} doesn't have the required TargetAddress component present!"
        );
        return;
    };

    debug!("Handling ConnectToServer event");

    commands.entity(client_entity).insert((
        ConnectionState::Connecting,
        TemporaryPeerId(next_temporary_peer_id.0),
    ));

    let Some(client_socket) = connect_to_server(target_address.0) else {
        error!("Failed to connect to server at {:?}", target_address.0);
        return;
    };

    let mut data = Vec::new();

    let byte_header = get_byte_header_for_datagram_type(DatagramType::NotifyInitialConnection);

    data.push(byte_header);
    data.extend_from_slice(&next_temporary_peer_id.0.to_be_bytes());

    client_socket
        .send(&data)
        .expect("Can send new connect message to server");

    debug!("Sending new connect message to server! {:?}", data);

    commands.insert_resource(ClientSocket(client_socket));

    next_temporary_peer_id.0 += 1;
}

struct ConfirmedNetEntityRequest {
    temporary_net_id: u8,
    net_entity_id: NetEntityId,
}

#[derive(Resource, Default)]
struct ConfirmedNetEntityRequestsQueue(pub Vec<ConfirmedNetEntityRequest>);

fn handle_data_client_socket(world: &mut World) {
    let client_socket = world.resource::<ClientSocket>();

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
                    net_entity_id: NetEntityId(net_entity_id),
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
                    let id = world.spawn(NetEntityId(*net_entity)).id();
                    debug!(
                        "Spawned Entity {id} for SyncExistingNetEntities with net_entity_id: {net_entity}"
                    )
                }
            }
            DatagramType::ComponentUpdate => {
                let Some(component_update) = get_component_update_from_datagram(&bytes) else {
                    debug!("Received invalid ComponentUpdate datagram: {:?}", bytes);
                    return;
                };
                let mut component_updates = world.resource_mut::<ComponentUpdatesToBeApplied>();
                component_updates.0.push(component_update);
            }
            DatagramType::AnnounceNewNetEntity => {
                let new_net_entity = NetEntityId(bytes[1]);

                debug!("Received AnnounceNewNetEntity. Spawning new entity for {new_net_entity:?}");

                world.spawn(new_net_entity);
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

                let mut query = world.query::<(Entity, &TemporaryPeerId)>();
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
                    .remove::<TemporaryPeerId>();

                world.insert_resource(OurPeerId(PeerId(peer_id)));

                debug!(
                    "Received ConfirmClientConnect from server, updated local entity and inserted OurPeerId resource!"
                );
            }
            DatagramType::NetworkMessage => match parse_u32_from_u8_arr(&bytes, 1, 5) {
                Ok(net_message_id) => {
                    let message_entry = {
                        world
                            .resource::<NetworkMessageRegistry>()
                            .message_entry
                            .get(&NetworkMessageId(net_message_id))
                            .copied()
                    };

                    let Some(message_entry) = message_entry else {
                        error!(
                            "Failed to find message_entry for incoming network message id {net_message_id:?} in registry"
                        );
                        return;
                    };

                    let net_message_handler = message_entry.server_to_client_message_handler;
                    let message_bytes = &bytes[5..];
                    net_message_handler(world, message_bytes);
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

                info!("Spawning a new Client because we received AnnounceNewClient message");
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
    query: Query<(Entity, Option<&TemporaryNetId>, Option<&NetEntityId>)>,
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

        debug!("Added confirmed {net_entity_id:?} from server into local entity {entity}");
    }
}

pub fn handle_new_replicate_entities_client(
    mut commands: Commands,
    query: Query<Entity, (Added<ReplicateEntity>, Without<NetEntityId>)>,
    mut next_temporary_net_entity_id: ResMut<NextTemporaryNetId>,
) {
    for added_entity in query {
        let temporary_net_id = TemporaryNetId(next_temporary_net_entity_id.0);
        debug!(
            "ReplicateEntity was added on entity {added_entity}, inserting {temporary_net_id:?}"
        );
        commands.entity(added_entity).insert(temporary_net_id);
        next_temporary_net_entity_id.0 += 1;
    }
}
