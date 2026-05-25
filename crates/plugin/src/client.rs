use bevy::prelude::*;

use crate::{
    CurrentSocket, SyncEntity,
    component_updates::{ComponentUpdates, get_component_update_from_datagram},
    net_entity::{
        NetEntity, NetEntityType, NextTemporaryNetId, TemporaryNetId,
        handle_new_temporary_net_entities,
    },
    network::connect_to_server,
    network_messages::{NetworkMessageId, NetworkMessageRegistry},
    sync_position::apply_internal_sync_position,
    util::{
        DatagramType, get_byte_header_for_datagram_type, get_datagram_type,
        parse_connect_to_server, parse_u32_from_u8_arr, receive_all_packets_from_socket,
    },
};

#[derive(States, PartialEq, Clone, Hash, Eq, Debug, Default)]
pub enum ClientConnectionState {
    #[default]
    None,
    Connecting,
    Connected,
}

/// Trigger this event on the client to connect to a server
#[derive(Event)]
pub struct ConnectToServer {
    pub server_url: String,
    pub port: u16,
}

/// Add this plugin on the client
pub struct NetvyClientPlugin;

impl Plugin for NetvyClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<ClientConnectionState>();

        app.init_resource::<ConfirmedNetEntityRequestsQueue>();

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
    mut next_connection_state: ResMut<NextState<ClientConnectionState>>,
) {
    debug!("Handling ConnectToServer event");
    next_connection_state.set(ClientConnectionState::Connecting);

    let address = parse_connect_to_server(trigger.event());

    let Some(client_socket) = connect_to_server(address) else {
        error!("Failed to connect to server at {address:?}");
        return;
    };

    let new_client_message = [get_byte_header_for_datagram_type(DatagramType::NewClient)];
    client_socket
        .send(&new_client_message)
        .expect("Can send new connect message to server");

    debug!(
        "Sending new connect message to server! {:?}",
        new_client_message
    );

    commands.insert_resource(CurrentSocket(client_socket));
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
                let mut next_connection_state =
                    world.resource_mut::<NextState<ClientConnectionState>>();
                next_connection_state.set(ClientConnectionState::Connected);
            }
            DatagramType::NetworkMessage => match parse_u32_from_u8_arr(&bytes, 1, 5) {
                Ok(network_message_id) => {
                    let network_message_registry = world.resource::<NetworkMessageRegistry>();
                    let Some(func) = network_message_registry
                        .message
                        .get(&NetworkMessageId(network_message_id))
                    else {
                        error!(
                            "Failed to find fn for incoming network message id {network_message_id:?} in registry"
                        );
                        return;
                    };
                    let message_bytes = &bytes[5..];
                    func(world, message_bytes);
                }
                Err(error) => {
                    error!("Failed to decode incoming network message: {error:?}");
                }
            },
            // A client doesnt receive these.
            DatagramType::ClientRequestNewNetEntity | DatagramType::NewClient => {}
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
    query: Query<Entity, Added<SyncEntity>>,
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
