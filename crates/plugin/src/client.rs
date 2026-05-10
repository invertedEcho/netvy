use bevy::prelude::*;

use crate::{
    ComponentRegistry, ComponentTypeId, ComponentUpdate, CurrentSocket, InternalSyncPosition,
    SyncEntity, SyncPosition, UpdateSequence,
    datagram::get_component_update_from_datagram,
    get_or_create_mut_update_sequence_number,
    net_entity::{
        NetEntityId, NetEntityType, NextTemporaryNetId, TemporaryNetId,
        handle_new_temporary_net_entities,
    },
    network::connect_to_server,
    util::{
        DatagramType, NEW_CLIENT_BYTE_HEADER, get_datagram_type, parse_connect_to_server,
        receive_all_packets_from_socket,
    },
};

struct FailedApplyComponentUpdate {
    pub component_type_id: ComponentTypeId,
    // We store the NetEntityId and not the Entity itself in case the update failed because of a
    // missing local entity (not yet spawned)
    pub net_entity_id: NetEntityId,
    pub component_bytes: Vec<u8>,
    incoming_update_sequence: u32,
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
                apply_internal_sync_position,
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

    commands.insert_resource(CurrentSocket(client_socket));
}

fn handle_data_client_socket(
    mut commands: Commands,
    client_socket: Res<CurrentSocket>,
    query: Query<(Entity, Option<&TemporaryNetId>, Option<&NetEntityId>)>,
    mut update_sequence: ResMut<UpdateSequence>,
    component_registry: Res<ComponentRegistry>,
    mut failed_component_updates: ResMut<FailedApplyComponentUpdates>,
    mut new_net_entity_message_writer: MessageWriter<NewNetEntityMessage>,
) {
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
            }
            DatagramType::SyncExistingNetEntities => {
                let net_entities = &bytes[1..];

                for net_entity in net_entities {
                    // TODO: Im only 99% sure that only other entities will be included in the
                    // IncomingNewNetEntity message. Very unlikely but still...
                    let id = commands
                        .spawn((NetEntityId(*net_entity), NetEntityType::Remote))
                        .id();
                    info!(
                        "Spawned Entity {id} for SyncExistingNetEntities with net_entity_id: {net_entity}"
                    )
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

                if let Some((existing_entity, _, _)) = query.iter().find(|res| {
                    let Some(res2) = res.2 else {
                        return false;
                    };
                    *res2 == net_entity_id
                }) {
                    let mut entity_commands = commands.entity(existing_entity);

                    let current_update_sequence = get_or_create_mut_update_sequence_number(
                        &mut update_sequence,
                        net_entity_id,
                        component_type_id,
                    );

                    if incoming_update_sequence <= *current_update_sequence {
                        info!(
                            "Not applying update, update is older or same as current update sequence"
                        );
                        return;
                    }

                    let succesful = apply_fn(&mut entity_commands, &component_bytes);
                    if succesful {
                        *current_update_sequence += 1;
                    }
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

                info!("Received AnnounceNewNetEntity. Spawning new entity for {new_net_entity:?}");

                commands.spawn((new_net_entity, NetEntityType::Remote));
            }
            // A client doesnt receive these.
            DatagramType::ClientRequestNewNetEntity | DatagramType::NewClient => {}
        }
    }
}

#[derive(Message)]
pub struct NewNetEntityMessage(pub NetEntityId);

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
    update_sequence: Res<UpdateSequence>,
    query: Query<(Entity, &NetEntityId)>,
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

// TODO: this would break if the user wants to run physics on entities with NetEntityType::Remote
fn apply_internal_sync_position(
    mut commands: Commands,
    query: Query<
        (
            Entity,
            Option<&mut Transform>,
            &mut InternalSyncPosition,
            &NetEntityType,
            &SyncPosition,
        ),
        Or<(Changed<Transform>, Changed<InternalSyncPosition>)>,
    >,
    time: Res<Time>,
) {
    for (entity, transform, mut internal_sync_position, entity_type, sync_position) in query {
        let x = internal_sync_position.x;
        let y = internal_sync_position.y;
        let z = internal_sync_position.z;

        if let Some(mut transform) = transform {
            match entity_type {
                NetEntityType::Local => {
                    internal_sync_position.x = transform.translation.x;
                    internal_sync_position.y = transform.translation.y;
                    internal_sync_position.z = transform.translation.z;
                }
                NetEntityType::Remote => {
                    if sync_position.linear_interpolation {
                        let current = transform.translation;
                        let target = vec3(x, y, z);
                        let lerp_factor = 10.0 * time.delta_secs();
                        info!("lerp_factor: {lerp_factor:?}");

                        transform.translation = current.lerp(target, lerp_factor);
                    } else {
                        transform.translation = vec3(x, y, z);
                    }
                }
            }
        } else {
            commands.entity(entity).insert(Transform::from_xyz(x, y, z));
        }
    }
}
