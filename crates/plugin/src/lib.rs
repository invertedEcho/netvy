use std::net::{SocketAddr, UdpSocket};

use crate::{
    client::{Client, ConnectionState, NetvyClientPlugin},
    component_updates::{
        ComponentUpdatePlugin, FailedSentComponentUpdates, UpdateSequenceMap,
        component_registry::ComponentTypeId,
    },
    net_entity::{NetEntityId, NextTemporaryNetId},
    network_messages::NetworkMessagePlugin,
    prelude::AppComponentExt,
    server::{NetvyServerPlugin, Server},
    sync_position::{InternalSyncPosition, SyncPosition, add_internal_sync_position_component},
};
use bevy::prelude::*;
use bincode::config::{self, BigEndian, Configuration};
use serde::{Deserialize, Serialize};

mod client;
mod component_updates;
mod net_entity;
mod network;
mod network_messages;
mod server;
mod sync_position;
mod util;

pub mod prelude {
    pub use crate::client::prelude::*;
    pub use crate::component_updates::prelude::*;
    pub use crate::network_messages::prelude::*;
    pub use crate::server::prelude::*;
    pub use crate::sync_position::SyncPosition;
    pub use crate::{
        AppType, Authority, NetvyPlugin, OurPeerId, Owned, OwnedBy, PeerId, ReplicateEntity,
        TargetAddress,
    };
}

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

// We need to differentiate between client and server because in AppType::ClientAndServer, we have
// both client and server running at same time and thus also need (at least) two sockets
#[derive(Resource)]
pub struct ClientSocket(pub UdpSocket);

#[derive(Resource)]
pub struct ServerSocket(pub UdpSocket);

#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub enum AppType {
    Client,
    Server,
    /// A client that also hosts a server for local-only purposes. Useful for games that also offer
    /// a multiplayer game and dont want seperate logic for singleplayer and multiplayer (highly recommended)
    HostClient,
}

/// Add this component to entities that should be replicated to other clients.
/// This component is the bare minimum and always required for an entity to be taken into
/// consideration by netvy.
/// Upon adding this component, netvy will add a NetEntityId component into this entity, that
/// identifies the entity across all clients. The NetEntityId will always be the same across clients.
#[derive(Component)]
pub struct ReplicateEntity;

/// For initial connection from client to server. Server will generate a "real" peer id and sent
/// it back to the client, alongside with this TemporaryPeerId, so the client app knows to which
/// client it should update the client id
#[derive(Component)]
pub struct TemporaryPeerId(u32);

/// Identifies a client or a server across clients and servers
#[derive(Component, Reflect, Eq, Hash, PartialEq, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct PeerId(pub u32);

// TODO: its not ideal that this component has very different behaviour depending on client or server.
/// On the client, this is used to determine to which server it should connect to.
/// On the server, it is used to specify to where the socket should be bind to.
#[derive(Component, Reflect)]
pub struct TargetAddress(pub SocketAddr);

/// You can insert this component into entities so you can know which peer owns this entity (from
/// the gameplay perspective).
///
/// If you want to know which peer actually owns / has authority over an entity, use the `Authority`
/// component.
///
/// For example, you have many players, and want to find the player for a certain client. You can
/// query for this component and compare the PeerId with the wanted client/peer.
///
/// This component is replicated to all connected clients.
#[derive(Component, Serialize, Deserialize, Debug, Reflect)]
pub struct OwnedBy(pub PeerId);

/// You can filter by this component on any replicated entity to only get entities that the
/// local, current client owns. Netvy automatically inserts this component for you, as long as you
/// insert the `OwnedBy` component into the corresponding entities.
#[derive(Component)]
pub struct Owned;

/// This component is used to determine which peer has authority over the entity.
///
/// For example, server-side spawned entities will have Authority compnent with peer id of the server.
///
/// Only peers that also have authority of a net entity will send component updates.
///
/// 1. If a server spawns an entity, it automatically gets authority over this entity.
/// 2. When a client requests spawning a new net entity, it will also get authority over
///    this entity. Authority is given by server after validing the request of spawning a new NetEntity.
#[derive(Component, Serialize, Deserialize, Debug)]
pub struct Authority(pub PeerId);

/// Trigger this event to start host-client mode.
/// This is needed for example if you want to have a Singleplayer mode, but dont want seperate logic
/// for server and the client.
/// This will start a client and a server at 127.0.0.1 and the specfied ports.
#[derive(Event)]
pub struct StartHostClient {
    pub client_port: u16,
    pub server_port: u16,
}

/// Add this plugin and specify whether this is a client or a server
/// Depending on the given `AppType`, specific systems will run
pub struct NetvyPlugin(pub AppType);

/// Configure various behaviour of netvy via this resource.
/// Insert this resource before you add the NetvyPlugin, so changes are applied from the start on.
#[derive(Resource)]
pub struct NetvyConfiguration {
    /// Whether netvy should insert the bevy `Name` component into netvy entities, such as a `Client`
    /// Per default, this is on.
    add_debug_names: bool,
}

impl Default for NetvyConfiguration {
    fn default() -> Self {
        Self {
            add_debug_names: true,
        }
    }
}

/// Retrieve this resource to determine which client/server is yours, in the current bevy world, using the PeerId in this resource.
///
/// Please note that this resource won't exist if you are running netvy in host-client mode, as both
/// client and server exist in the same bevy world. In host-client mode you only have one client and
/// one server anyways, so you shouldn't need this resource anyways.
///
/// netvy automatically sets up this resource for you. Note that you may want to use Option<Res<>>,
/// as the resource may not exist yet if a client/server didn't yet fully established connection.
#[derive(Resource, Debug)]
pub struct OurPeerId(pub PeerId);

impl Plugin for NetvyPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0);

        app.init_resource::<NextTemporaryNetId>();
        app.init_resource::<FailedSentComponentUpdates>();
        app.init_resource::<UpdateSequenceMap>();

        // using init we ensure we dont overwrite any configuration made by the user.
        app.init_resource::<NetvyConfiguration>();

        app.add_plugins(NetworkMessagePlugin);
        app.add_plugins(ComponentUpdatePlugin);

        match self.0 {
            AppType::Client => {
                app.add_plugins(NetvyClientPlugin);
            }
            AppType::Server => {
                app.add_plugins(NetvyServerPlugin);
            }
            AppType::HostClient => {
                app.add_plugins(NetvyClientPlugin);
                app.add_plugins(NetvyServerPlugin);
            }
        }

        app.register_component::<InternalSyncPosition>();
        app.register_component_with_sync_mode::<SyncPosition>(SyncMode::OnChange);
        app.register_component_with_sync_mode::<OwnedBy>(SyncMode::OnChange);
        app.register_component_with_sync_mode::<Authority>(SyncMode::OnChange);

        app.add_systems(
            FixedUpdate,
            (
                add_internal_sync_position_component,
                add_debug_name_to_clients,
                add_debug_name_to_servers,
                add_owned,
            ),
        );

        // app.add_observer(handle_start_host_client);

        if cfg!(debug_assertions) {
            app.register_type::<NetEntityId>()
                .register_type::<InternalSyncPosition>()
                .register_type::<UpdateSequenceMap>()
                .register_type::<PeerId>()
                .register_type::<ConnectionState>()
                .register_type::<TargetAddress>()
                .register_type::<OwnedBy>();
        }
    }
}

fn get_or_create_mut_update_sequence_number(
    update_sequence: &mut UpdateSequenceMap,
    net_entity_id: NetEntityId,
    component_type_id: ComponentTypeId,
) -> &mut u32 {
    update_sequence
        .0
        .entry((net_entity_id, component_type_id))
        .or_insert(0)
}

fn add_debug_name_to_clients(
    mut commands: Commands,
    query: Query<Entity, Added<Client>>,
    netvy_configuration: Res<NetvyConfiguration>,
) {
    if !netvy_configuration.add_debug_names {
        return;
    }

    for entity in query {
        commands.entity(entity).insert(Name::new("Client"));
    }
}

fn add_debug_name_to_servers(
    mut commands: Commands,
    query: Query<Entity, Added<Server>>,
    netvy_configuration: Res<NetvyConfiguration>,
) {
    if !netvy_configuration.add_debug_names {
        return;
    }

    for entity in query {
        commands.entity(entity).insert(Name::new("Server"));
    }
}

// fn handle_start_host_client(trigger: On<StartHostClient>, mut commands: Commands) {
//     info!("handling host client");
//     let server = commands
//         .spawn((
//             Server,
//             TargetAddress {
//                 address: "127.0.0.1".to_string(),
//                 port: trigger.server_port,
//             },
//         ))
//         .id();
//     commands.trigger(StartServer {
//         server_entity: server,
//     });
//     info!("triggered start server");
//
//     let client = commands
//         .spawn((
//             Client,
//             TargetAddress {
//                 address: "127.0.0.1".to_string(),
//                 port: trigger.client_port,
//             },
//         ))
//         .id();
//     commands.trigger(ConnectToServer {
//         client_entity: client,
//     });
//     info!("triggered start client");
// }

fn add_owned(
    mut commands: Commands,
    query: Query<(Entity, &OwnedBy), Added<OwnedBy>>,
    our_peer_id: Option<Res<OurPeerId>>,
) {
    for (entity, owned_by) in query {
        debug!("OwnedBy was added, checking if this our entity. ({our_peer_id:?}, {owned_by:?})");
        // NOTE: has to be in the for loop, so it only runs when OwnedBy was added on any entity
        let Some(ref our_peer_id) = our_peer_id else {
            warn!(
                "Can't check if this entity should have Owned, OurPeerId resource doesn't exist yet."
            );
            continue;
        };
        if owned_by.0 == our_peer_id.0 {
            commands.entity(entity).insert(Owned);
        }
    }
}
