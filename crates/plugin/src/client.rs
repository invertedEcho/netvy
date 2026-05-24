use std::time::Duration;

use bevy::{prelude::*, time::common_conditions::on_timer};

use crate::{
    CurrentSocket, SyncEntity,
    component_registry::ComponentRegistry,
    component_updates::{
        ComponentUpdatePlugin, ComponentUpdates, FailedApplyComponentUpdates, UpdateSequenceMap,
        get_component_update_from_datagram,
    },
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
pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<ClientConnectionState>();

        app.add_observer(handle_connect_trigger);

        app.add_message::<NewNetEntityMessage>();

        app.init_resource::<FailedApplyComponentUpdates>();

        app.add_plugins(ComponentUpdatePlugin);

        app.add_systems(
            Update,
            (
                handle_data_client_socket,
                handle_new_net_entity_message,
                handle_new_temporary_net_entities,
                handle_failed_component_updates.run_if(on_timer(Duration::from_secs_f32(1.0))),
                apply_internal_sync_position,
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

fn handle_data_client_socket(
    mut commands: Commands,
    client_socket: If<Res<CurrentSocket>>,
    mut component_updates: ResMut<ComponentUpdates>,
    query: Query<(Entity, Option<&TemporaryNetId>, Option<&NetEntity>)>,
    mut next_connection_state: ResMut<NextState<ClientConnectionState>>,
    network_message_registry: Res<NetworkMessageRegistry>,
) {
    for (bytes, _) in receive_all_packets_from_socket(&client_socket.0.0) {
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
                let datagram_temporary_net_id = bytes[1];
                let entity = query
                    .iter()
                    .find(|(_, temporary_net_id, _)| {
                        let Some(temporary_net_id) = temporary_net_id else {
                            return false;
                        };
                        temporary_net_id.0 == datagram_temporary_net_id
                    })
                    .map(|(entity, _, _)| entity);

                let Some(entity) = entity else {
                    error!(
                        "Received a CONFIRM_NEW_NET_ENTITY message from server but couldnt find any entity that matches the temporary net id from datagram: {}",
                        datagram_temporary_net_id
                    );
                    return;
                };

                let net_entity_id = bytes[2];
                let mut entity_commands = commands.entity(entity);

                let net_entity_id = NetEntity(net_entity_id);
                entity_commands.insert(net_entity_id);
                entity_commands.remove::<TemporaryNetId>();

                info!("Added confirmed {net_entity_id:?} from server into local entity {entity}");
            }
            DatagramType::SyncExistingNetEntities => {
                let net_entities = &bytes[1..];

                for net_entity in net_entities {
                    // TODO: Im only 99% sure that only other entities will be included in the
                    // IncomingNewNetEntity message. Very unlikely but still...
                    let id = commands
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
                component_updates.0.push(component_update);
            }
            DatagramType::AnnounceNewNetEntity => {
                let new_net_entity = NetEntity(bytes[1]);

                info!("Received AnnounceNewNetEntity. Spawning new entity for {new_net_entity:?}");

                commands.spawn((new_net_entity, NetEntityType::Remote));
            }
            DatagramType::ConfirmClientConnect => {
                next_connection_state.set(ClientConnectionState::Connected);
            }
            DatagramType::NetworkMessage => match parse_u32_from_u8_arr(&bytes, 1, 4) {
                Ok(network_message_id) => {
                    let Some(func) = network_message_registry
                        .message
                        .get(&NetworkMessageId(network_message_id))
                    else {
                        error!(
                            "Failed to find fn for incoming network message id {network_message_id:?} in registry"
                        );
                        return;
                    };
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

#[derive(Message)]
pub struct NewNetEntityMessage(pub NetEntity);

// TODO: I'm not sure whether i wanna keep this message. We already have AnnounceNewNetEntity. but
// this is like a backup plan in case we didnt receive AnnounceNewNetEntity
pub fn handle_new_net_entity_message(
    mut commands: Commands,
    mut message_reader: MessageReader<NewNetEntityMessage>,
) {
    for message in message_reader.read() {
        info!("Received NewNetEntityMessage, spawning local entity for new NetEntityId!");
        commands.spawn(message.0);
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

#[derive(Resource)]
struct FailedComponentUpdatesTimer(pub Timer);

fn handle_failed_component_updates_timer(
    mut timer: ResMut<FailedComponentUpdatesTimer>,
    time: Res<Time>,
) {
    timer.0.tick(time.delta());
}

fn handle_failed_component_updates(
    mut commands: Commands,
    failed_component_updates_timer: Res<FailedComponentUpdatesTimer>,
    mut failed_component_updates: ResMut<FailedApplyComponentUpdates>,
    component_registry: Res<ComponentRegistry>,
    update_sequence: Res<UpdateSequenceMap>,
    query: Query<(Entity, &NetEntity)>,
) {
    if !failed_component_updates_timer.0.is_finished() {
        return;
    }
    failed_component_updates
        .0
        .retain(|failed_component_update| {
            let component_type_id = &failed_component_update.component_type_id;
            let net_entity_id = &failed_component_update.net_entity_id;
            let Some(apply_fn) = component_registry.apply.get(component_type_id) else {
                return true;
            };
            let Some(entity) = query
                .iter()
                .find(|(_, net_entity_id)| **net_entity_id == failed_component_update.net_entity_id)
                .map(|(entity, _)| entity)
            else {
                return true;
            };

            let Some(current_update_sequence) =
                update_sequence.0.get(&(*net_entity_id, *component_type_id))
            else {
                warn!("Failed to get current update sequence");
                return true;
            };

            if failed_component_update.incoming_update_sequence <= *current_update_sequence {
                info!("Not applying update, update is older or same as current update sequence");
                return true;
            }

            let mut entity_commands = commands.entity(entity);

            apply_fn(
                &mut entity_commands,
                &failed_component_update.component_bytes,
            );
            false
        });
}
