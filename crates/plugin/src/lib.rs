use std::{net::UdpSocket, time::Duration};

use crate::{
    client::{ClientPlugin, handle_new_sync_entities},
    component_updates::{FailedSentComponentUpdates, UpdateSequence, handle_send_interval_timer},
    datagram::build_component_update_datagram,
    net_entity::{NetEntity, NetEntityType, NextTemporaryNetId},
    registry::{AppComponentExt, ComponentRegistry, ComponentTypeId, NextComponentTypeId},
    server::{NextNetEntityId, ServerPlugin},
    sync_position::{InternalSyncPosition, SyncPosition, add_internal_sync_position_component},
};
use bevy::{prelude::*, time::common_conditions::on_timer};
use bincode::config::{self, BigEndian, Configuration};

// TODO: At some point we probably want to re-export specific stuff instead of everything
pub mod client;
pub mod component_updates;
mod datagram;
mod messages;
pub mod net_entity;
pub mod network;
pub mod registry;
pub mod server;
pub mod sync_position;
mod util;

const BINCODE_CONFIG: Configuration<BigEndian> = config::standard().with_big_endian();

pub enum SyncMode {
    /// Sends component updates every x seconds (right now even if unchanged)
    FixedRate(f32),
    /// Sends component updates whenever the component changes
    OnChange,
}

impl Default for SyncMode {
    fn default() -> Self {
        Self::FixedRate(0.05)
    }
}

/// The socket of the current running instance (server or client)
#[derive(Resource)]
pub struct CurrentSocket(pub UdpSocket);

#[derive(Resource, Clone, Copy, PartialEq)]
pub enum AppType {
    Client,
    Server,
}

#[derive(Debug)]
struct ComponentUpdate {
    net_entity_id: NetEntity,
    component_type_id: ComponentTypeId,
    component_bytes: Vec<u8>,
    update_sequence: u32,
}

/// Add this component to entities that should be synced across clients.
/// This component is the bare minimum and always required for an entity to be taken into
/// consideration by netvy.
/// Upon adding this component, netvy will add a NetEntityId component into this entity, that
/// identifies the entity across all clients. The NetEntityId will always be the same across clients.
#[derive(Component)]
pub struct SyncEntity;

/// Add this plugin and specify whether this is a client or a server
/// Depending on the given `AppType`, specific systems will run
pub struct NetvyPlugin(pub AppType);

impl Plugin for NetvyPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0);

        app.init_resource::<ComponentRegistry>();
        app.init_resource::<NextComponentTypeId>();
        app.init_resource::<NextNetEntityId>();
        app.init_resource::<NextTemporaryNetId>();
        app.init_resource::<FailedSentComponentUpdates>();
        app.init_resource::<UpdateSequence>();

        match self.0 {
            AppType::Client => {
                app.add_plugins(ClientPlugin);
            }
            AppType::Server => {
                app.add_plugins(ServerPlugin);
            }
        }

        app.register_component::<InternalSyncPosition>();
        app.register_component_with_sync_mode::<SyncPosition>(SyncMode::OnChange);

        app.add_systems(
            Update,
            (
                add_entity_type_to_sync_entities,
                add_internal_sync_position_component,
                handle_new_sync_entities,
                handle_failed_sent_component_updates.run_if(on_timer(Duration::from_secs_f32(1.0))),
                handle_send_interval_timer,
            ),
        );

        if cfg!(debug_assertions) {
            app.register_type::<NetEntity>()
                .register_type::<InternalSyncPosition>()
                .register_type::<NetEntityType>()
                .register_type::<UpdateSequence>();
        }
    }
}

// All entities that have SyncEntity component are local entities
fn add_entity_type_to_sync_entities(
    mut commands: Commands,
    query: Query<Entity, Added<SyncEntity>>,
) {
    for entity in query {
        commands.entity(entity).insert(NetEntityType::Local);
    }
}

fn handle_failed_sent_component_updates(
    mut resource: ResMut<FailedSentComponentUpdates>,
    entities: Query<&NetEntity>,
    current_socket: If<Res<CurrentSocket>>,
    mut update_sequence: ResMut<UpdateSequence>,
) {
    resource.0.retain(|failed_component_update| {
        let Ok(net_entity_id) = entities.get(failed_component_update.entity) else {
            info!("still cant apply failed component update, no matching entity");
            return true;
        };

        let current_update_sequence = get_or_create_mut_update_sequence_number(
            &mut update_sequence,
            *net_entity_id,
            failed_component_update.component_type_id,
        );

        *current_update_sequence += 1;

        let data = build_component_update_datagram(
            &failed_component_update.component_bytes,
            failed_component_update.component_type_id,
            net_entity_id,
            *current_update_sequence,
        );

        // dont retain if sending was succesful
        !current_socket.0.0.send(&data).is_ok()
    });
}

fn get_or_create_mut_update_sequence_number(
    update_sequence: &mut UpdateSequence,
    net_entity_id: NetEntity,
    component_type_id: ComponentTypeId,
) -> &mut u32 {
    update_sequence
        .0
        .entry((net_entity_id, component_type_id))
        .or_insert(0)
}
