use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;

use crate::{
    ComponentRegistry, ComponentTypeId, ComponentUpdate, SyncEntity, UpdateSequence,
    datagram::get_component_update_from_datagram,
    net_entity::{
        NetEntityId, NetEntityMapping, NetEntityType, NextTemporaryNetId, TemporaryNetId,
        handle_new_temporary_net_entities,
    },
    network::connect_to_server,
    util::{
        DatagramType, NEW_CLIENT_BYTE_HEADER, get_datagram_type, parse_connect_to_server,
        receive_bytes_from_socket,
    },
};

struct FailedApplyComponentUpdate {
    pub component_type_id: ComponentTypeId,
    // We store the NetEntityId and not the Entity itself in case the update failed because of a
    // missing local entity (not yet spawned)
    pub net_entity_id: NetEntityId,
    pub component_bytes: Vec<u8>,
    incoming_update_sequence: UpdateSequence,
}

/// Stores component updates that failed to apply locally, for example no entity exists yet with the
/// given `net_entity_id`
#[derive(Resource, Default)]
struct FailedApplyComponentUpdates(Vec<FailedApplyComponentUpdate>);

/// Trigger this event on the client to connect to a server
#[derive(Event)]
pub struct ConnectToServer {
    pub server_url: String,
    pub port: u16,
}

/// The socket of the current client
#[derive(Resource)]
pub struct CurrentClientSocket(pub UdpSocket);

/// Add this plugin on the client
pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_connect_trigger);

        app.add_message::<NewNetEntityMessage>();

        app.insert_resource(FailedComponentUpdatesTimer(Timer::from_seconds(
            1.0,
            TimerMode::Repeating,
        )));
        app.init_resource::<FailedApplyComponentUpdates>();

        app.add_systems(
            Update,
            (
                handle_data_client_socket,
                handle_new_net_entity_message,
                handle_new_temporary_net_entities,
                handle_failed_component_updates_timer,
                handle_failed_component_updates,
            ),
        );
    }
}

fn handle_connect_trigger(trigger: On<ConnectToServer>, mut commands: Commands) {
    debug!("Handling ConnectToServer event");
    let address = parse_connect_to_server(trigger.event());

    let client_socket = connect_to_server(address);

    let new_client_message = [NEW_CLIENT_BYTE_HEADER];
    client_socket
        .send(&new_client_message)
        .expect("Can send new connect message to server");

    debug!(
        "Sending new connect message to server! {:?}",
        new_client_message
    );

    commands.insert_resource(CurrentClientSocket(client_socket));
}

fn handle_data_client_socket(
    mut commands: Commands,
    client_socket: Res<CurrentClientSocket>,
    // but we remove TemporaryNetId at some point?
    query: Query<(Entity, Option<&TemporaryNetId>, Option<&UpdateSequence>)>,
    mut net_entity_mapping: ResMut<NetEntityMapping>,
    component_registry: Res<ComponentRegistry>,
    mut failed_component_updates: ResMut<FailedApplyComponentUpdates>,
    mut new_net_entity_message_writer: MessageWriter<NewNetEntityMessage>,
) {
    let Some((bytes, _)) = get_bytes_from_client_socket(&client_socket.0) else {
        return;
    };

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

            let net_entity_id = NetEntityId(net_entity_id);
            entity_commands.insert(net_entity_id);
            entity_commands.remove::<TemporaryNetId>();

            info!("Added confirmed {net_entity_id:?} from server into local entity {entity}");
            net_entity_mapping.0.insert(net_entity_id, entity);
        }
        DatagramType::SyncExistingNetEntities => {
            let net_entities = &bytes[1..];
            info!("Spawning local entities for received new net entities {net_entities:?}!");
            for net_entity in net_entities {
                // TODO: Im only 99% sure that only other entities will be included in the
                // IncomingNewNetEntity message. Very unlikely but still...
                let net_entity_id = NetEntityId(*net_entity);

                let entity = commands.spawn((net_entity_id, NetEntityType::Remote)).id();
                net_entity_mapping.0.insert(net_entity_id, entity);
            }
        }
        DatagramType::ComponentUpdate => {
            let Some(ComponentUpdate {
                net_entity_id,
                component_type_id,
                component_bytes,
                update_sequence: incoming_update_sequence,
            }) = get_component_update_from_datagram(&bytes)
            else {
                return;
            };

            let apply_fn = {
                let Some(apply_fn) = component_registry.apply.get(&component_type_id) else {
                    error!("Failed to find apply_fn for internal_type_id: {component_type_id}");
                    return;
                };
                *apply_fn
            };

            if let Some(existing_entity) = net_entity_mapping.0.get(&net_entity_id) {
                let Ok((_, _, maybe_update_sequence)) = query.get(*existing_entity) else {
                    error!("Failed to find entity");
                    return;
                };

                let mut entity_commands = commands.entity(*existing_entity);

                let existing_update_sequence = match maybe_update_sequence {
                    Some(update_sequence) => *update_sequence,
                    None => {
                        let update_sequence = UpdateSequence { last_sequence: 0 };
                        entity_commands.insert(update_sequence);
                        update_sequence
                    }
                };

                apply_fn(
                    &mut entity_commands,
                    &component_bytes,
                    &existing_update_sequence,
                    &incoming_update_sequence,
                );
            } else {
                info!("Adding component update to FailedComponentUpdates");
                failed_component_updates.0.push(FailedApplyComponentUpdate {
                    net_entity_id,
                    component_bytes,
                    component_type_id,
                    incoming_update_sequence,
                });
                new_net_entity_message_writer.write(NewNetEntityMessage(net_entity_id));
            }
        }
        DatagramType::AnnounceNewNetEntity => {
            let new_net_entity = NetEntityId(bytes[1]);

            if net_entity_mapping.0.contains_key(&new_net_entity) {
                info!(
                    "Received AnnounceNewNetEntity but an Entity already exists for the given {new_net_entity:?}"
                );
                return;
            }

            info!("Received AnnounceNewNetEntity. Spawning new entity for {new_net_entity:?}");

            let new_entity = commands.spawn((new_net_entity, NetEntityType::Remote)).id();
            net_entity_mapping.0.insert(new_net_entity, new_entity);
        }
        // A client doesnt receive these.
        DatagramType::ClientRequestNewNetEntity | DatagramType::NewClient => {}
    }
}

#[derive(Message)]
pub struct NewNetEntityMessage(pub NetEntityId);

// TODO: I'm not sure whether i wanna keep this message. We already have AnnounceNewNetEntity. but
// this is like a backup plan in case we didnt receive AnnounceNewNetEntity
pub fn handle_new_net_entity_message(
    mut commands: Commands,
    mut message_reader: MessageReader<NewNetEntityMessage>,
    mut net_entity_mapping: ResMut<NetEntityMapping>,
) {
    for message in message_reader.read() {
        if net_entity_mapping.0.contains_key(&message.0) {
            continue;
        }

        info!(
            "Received NewNetEntityMessage and NetEntityId not in our NetEntityMapping, spawning local entity for new NetEntityId!"
        );
        let entity_id = commands.spawn_empty().id();
        net_entity_mapping.0.insert(message.0, entity_id);
    }
}

fn get_bytes_from_client_socket(socket: &UdpSocket) -> Option<(Vec<u8>, SocketAddr)> {
    receive_bytes_from_socket(socket)
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
    net_entity_mapping: Res<NetEntityMapping>,
    query: Query<&UpdateSequence>,
) {
    if !failed_component_updates_timer.0.is_finished() {
        return;
    }
    failed_component_updates
        .0
        .retain(|failed_component_update| {
            let Some(apply_fn) = component_registry
                .apply
                .get(&failed_component_update.component_type_id)
            else {
                return true;
            };
            let Some(entity) = net_entity_mapping
                .0
                .get(&failed_component_update.net_entity_id)
            else {
                return true;
            };

            let current_update_sequence = query.get(*entity).unwrap();

            let mut entity_commands = commands.entity(*entity);

            apply_fn(
                &mut entity_commands,
                &failed_component_update.component_bytes,
                current_update_sequence,
                &failed_component_update.incoming_update_sequence,
            );
            false
        });
}
