use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;

use crate::{
    ComponentRegistry, SyncEntity,
    net_entity::{
        CONFIRM_NEW_NET_ENTITY_BYTE_HEADER, NEW_NET_ENTITY_BYTE_HEADER, NetEntityId,
        NetEntityMapping, NextTemporaryNetId, TemporaryNetId, handle_new_temporary_net_entities,
    },
    network::connect_to_server,
    util::{
        extract_component_type_id, extract_net_entity_id, parse_connect_to_server,
        receive_bytes_from_socket,
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

// we should have a message that the server receives that tells the server its a new connect from a
// client
fn handle_connect_trigger(trigger: On<ConnectToServer>, mut commands: Commands) {
    debug!("Handling ConnectToServer event");
    let address = parse_connect_to_server(trigger.event());

    let client_socket = connect_to_server(address);

    info!("Sending new connect message to server!");
    client_socket
        .send(&[1])
        .expect("Can send new connect message to server");

    commands.insert_resource(CurrentClientSocket(client_socket));
}

fn handle_data_client_socket(world: &mut World) {
    let Some((bytes, _)) = get_bytes_from_client_socket(world) else {
        return;
    };

    if bytes.starts_with(&[CONFIRM_NEW_NET_ENTITY_BYTE_HEADER]) {
        let temporary_net_id = bytes[1];
        let mut query = world.query::<(Entity, &TemporaryNetId)>();

        let matching_entities = query
            .iter(world)
            .find(|(_, temp_net_id)| temp_net_id.0 == temporary_net_id)
            .map(|(entity, _)| entity);

        if let Some(entity) = matching_entities {
            let net_entity_id = bytes[2];
            let mut entity_commands = world.entity_mut(entity);
            entity_commands.insert(NetEntityId(net_entity_id));
            info!(
                "Added NetEntityId {} confirmed from server into local entity {}",
                net_entity_id, entity
            );
        } else {
            error!(
                "Received a CONFIRM_NEW_NET_ENTITY message from server but couldnt find any entity that matches the temporary net id from datagram: {}",
                temporary_net_id
            );
        }
    } else if bytes.starts_with(&[NEW_NET_ENTITY_BYTE_HEADER]) {
        let net_entities = &bytes[1..bytes.len() - 1];
        for net_entity in net_entities {
            info!(
                "Spawning entity with net entity for new net entity {}, notified from server.",
                net_entity
            );
            world.spawn(NetEntityId(*net_entity));
        }
    } else {
        // We assume this is just a normal component update. I think we should do this
        // differently. First byte should be what type of message is this?
        // first byte is internal type id
        let Some(internal_type_id_bytes) = extract_component_type_id(&bytes) else {
            error!("Couldnt extract internal component type id");
            return;
        };

        let apply_fn = {
            let Some(component_registry) = world.get_resource::<ComponentRegistry>() else {
                return;
            };
            let Some(apply_fn) = component_registry.apply.get(&internal_type_id_bytes) else {
                return;
            };
            *apply_fn
        };

        let Some(extracted_net_entity_id) = extract_net_entity_id(&bytes) else {
            warn!(
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
            // NOTE: This will mean this current component update wont be done, only spawning the new
            // entity, but the next one will be
            world.write_message(NewNetEntityMessage(extracted_net_entity_id));
        }
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
        info!("Spawning local entity for new NetEntityId!");
        let entity_id = commands.spawn_empty().id();
        net_entity_mapping.0.insert(message.0.clone(), entity_id);
    }
}

fn get_bytes_from_client_socket(world: &World) -> Option<(Vec<u8>, SocketAddr)> {
    let socket = world.resource::<CurrentClientSocket>();
    receive_bytes_from_socket(&socket.0)
}

pub fn request_net_entity(
    mut commands: Commands,
    query: Query<Entity, Added<SyncEntity>>,
    mut next_temporary_net_entity_id: ResMut<NextTemporaryNetId>,
) {
    for added_entity in query {
        info!("Added<SyncEntity>! Adding TemporaryNetId");
        commands
            .entity(added_entity)
            .insert(TemporaryNetId(next_temporary_net_entity_id.0));
        next_temporary_net_entity_id.0 += 1;
    }
}
