use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;

use crate::{
    ComponentRegistry, SyncEntity,
    net_entity::{
        EntityType, NetEntityId, NetEntityMapping, NextTemporaryNetId, TemporaryNetId,
        handle_new_temporary_net_entities,
    },
    network::connect_to_server,
    util::{
        DatagramType, NEW_CLIENT_BYTE_HEADER, extract_component_type_id, extract_net_entity_id,
        get_datagram_type, parse_connect_to_server, receive_bytes_from_socket,
    },
};

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

        app.add_systems(
            Update,
            (
                handle_data_client_socket,
                handle_new_net_entity_message,
                handle_new_temporary_net_entities,
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

    info!(
        "Sending new connect message to server! {:?}",
        new_client_message
    );

    commands.insert_resource(CurrentClientSocket(client_socket));
}

fn handle_data_client_socket(world: &mut World) {
    let Some((bytes, _)) = get_bytes_from_client_socket(world) else {
        return;
    };

    let Some(datagram_type) = get_datagram_type(&bytes) else {
        return;
    };

    match datagram_type {
        DatagramType::ConfirmNetEntityRequest => {
            if bytes.len() < 2 {
                error!(
                    "Received a ConfirmNewNetEntity message without net id, datagram: {bytes:?}"
                );
                return;
            }
            let temporary_net_id = bytes[1];
            let mut query = world.query::<(Entity, &TemporaryNetId)>();

            let matching_entities = query
                .iter(world)
                .find(|(_, temp_net_id)| temp_net_id.0 == temporary_net_id)
                .map(|(entity, _)| entity);

            if let Some(entity) = matching_entities {
                let net_entity_id = bytes[2];
                let mut entity_commands = world.entity_mut(entity);

                let net_entity_id = NetEntityId(net_entity_id);
                entity_commands.insert(net_entity_id.clone());
                entity_commands.remove::<TemporaryNetId>();

                info!("Added confirmed {net_entity_id:?} from server into local entity {entity}");
                world
                    .resource_mut::<NetEntityMapping>()
                    .0
                    .insert(net_entity_id, entity);
            } else {
                error!(
                    "Received a CONFIRM_NEW_NET_ENTITY message from server but couldnt find any entity that matches the temporary net id from datagram: {}",
                    temporary_net_id
                );
            }
        }
        DatagramType::SyncExistingNetEntities => {
            let net_entities = &bytes[1..];
            info!(
                "Received datagram for new net entities! Spawning local entities. All bytes: {bytes:?} our slice: {net_entities:?}"
            );
            for net_entity in net_entities {
                // TODO: Im only 99% sure that only other entities will be included in the
                // IncomingNewNetEntity message. Very unlikely but still...
                world.spawn((NetEntityId(*net_entity), EntityType::Remote));
            }
        }
        DatagramType::ComponentUpdate => {
            let Some(component_type_id) = extract_component_type_id(&bytes) else {
                error!("Couldnt extract component type id");
                return;
            };

            let apply_fn = {
                let component_registry = world
                    .get_resource::<ComponentRegistry>()
                    .expect("ComponentRegistry must exist");

                let Some(apply_fn) = component_registry.apply.get(&component_type_id) else {
                    error!("Failed to find apply_fn for internal_type_id: {component_type_id}");
                    return;
                };
                *apply_fn
            };

            let Some(extracted_net_entity_id) = extract_net_entity_id(&bytes) else {
                error!(
                    "Received datagram that doesnt contain a NetEntityId. Datagram: {:?}",
                    bytes
                );
                return;
            };

            if let Some(existing_entity) = world
                .resource::<NetEntityMapping>()
                .0
                .get(&extracted_net_entity_id)
            {
                apply_fn(world, *existing_entity, &bytes);
            } else {
                // TODO: This will mean this current component update wont be done, only spawning the new
                // entity, but the next one will be
                world.write_message(NewNetEntityMessage(extracted_net_entity_id));
            }
        }
        DatagramType::AnnounceNewNetEntity => {
            let new_net_entity = NetEntityId(bytes[1]);

            if world
                .resource::<NetEntityMapping>()
                .0
                .contains_key(&new_net_entity)
            {
                info!(
                    "Received AnnounceNewNetEntity but an Entity already exists for the given {new_net_entity:?}"
                );
                return;
            }

            info!("Received AnnounceNewNetEntity. Spawning new entity for {new_net_entity:?}");

            let new_entity = world
                .spawn((new_net_entity.clone(), EntityType::Remote))
                .id();
            world
                .resource_mut::<NetEntityMapping>()
                .0
                .insert(new_net_entity, new_entity);
        }
        // A client doesnt receive these.
        DatagramType::ClientRequestNewNetEntity | DatagramType::NewClient => {}
    }
}

#[derive(Message)]
pub struct NewNetEntityMessage(pub NetEntityId);

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
        net_entity_mapping.0.insert(message.0.clone(), entity_id);
    }
}

fn get_bytes_from_client_socket(world: &World) -> Option<(Vec<u8>, SocketAddr)> {
    let socket = world.resource::<CurrentClientSocket>();
    receive_bytes_from_socket(&socket.0)
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
