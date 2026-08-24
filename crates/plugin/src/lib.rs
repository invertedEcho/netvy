use std::net::{SocketAddr, UdpSocket};

use crate::{
    alive_check::AliveCheckPlugin,
    client::{Client, ConnectionState, NetvyClientPlugin},
    component_updates::{
        ComponentUpdatePlugin, FailedSentComponentUpdates, UpdateSequenceMap, UpdateSequenceNumber,
        component_registry::ComponentTypeId,
    },
    disconnect::DisconnectPlugin,
    net_entity::{NetEntityId, NextTemporaryNetId},
    network_messages::NetworkMessagePlugin,
    prelude::AppComponentExt,
    server::{NetvyServerPlugin, Server},
    sync_position::{InternalSyncPosition, SyncPosition, SyncPositionPlugin},
};
use bevy::prelude::*;
use bincode::config::{self, BigEndian, Configuration};
use serde::{Deserialize, Serialize};

mod alive_check;
mod client;
mod component_updates;
mod disconnect;
mod net_entity;
mod network;
mod network_messages;
mod server;
mod sync_position;
mod util;

pub mod prelude {
    pub use crate::client::prelude::*;
    pub use crate::component_updates::prelude::*;
    pub use crate::disconnect::prelude::*;
    pub use crate::net_entity::NetEntityId;
    pub use crate::network_messages::prelude::*;
    pub use crate::server::prelude::*;
    pub use crate::sync_position::prelude::*;
    pub use crate::{
        Authority, NetvyMode, NetvyPlugin, OurPeerId, Owned, Owner, PeerId, ReplicateEntity,
        TargetAddress,
    };
}

const BINCODE_CONFIG: Configuration<BigEndian> = config::standard().with_big_endian();

#[derive(Default)]
pub enum SyncMode {
    /// Sends component updates every x seconds (right now even if unchanged)
    FixedRate(f32),
    /// Sends component updates whenever the component changes
    #[default]
    OnChange,
}

// We need to differentiate between client and server because in AppType::ClientAndServer, we have
// both client and server running at same time and thus also need (at least) two sockets
#[derive(Resource)]
pub struct ClientSocket(pub UdpSocket);

#[derive(Resource)]
pub struct ServerSocket(pub UdpSocket);

#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub enum NetvyMode {
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
/// On the server, it is used to specify to where the socket should be bind to.
/// On the client, this is used to determine to which server it should connect to.
/// The client socket always binds to port 0, e.g. the system uses a random free port.
#[derive(Component, Reflect)]
pub struct TargetAddress(pub SocketAddr);

/// This component answers the question about which peer this entity belongs to. This is different
/// from `Authority`.
///
/// In order to avoid having to manually compare peer ids, you can filter by the `Owned` component
/// to only get entities that belong to the current peer.
#[derive(Component, Serialize, Deserialize, Debug, Reflect)]
pub struct Owner(pub PeerId);

/// You can filter by this component on any replicated entity to only get entities that the
/// current peer owns. Netvy automatically inserts this component for you, as long as you
/// insert the `Owner` component into the corresponding entities.
#[derive(Component)]
pub struct Owned;

/// This component is used to determine which peer has authority over the entity.
///
/// Authority means the ability to mutate state of an entity, e.g. its components
///
/// In order to avoid having to manually compare peer ids, you can filter by the `Authoritative` component,
/// to only get entities on which the current peer has authority over.
///
/// Note that the server can always mutate state of any entity, even if it doesn't have authority
/// over that entity.
/// If you have a valid use-case where you would not like this to happen, please open an issue in
/// the github repository.
#[derive(Component, Serialize, Deserialize, Debug, Reflect)]
pub struct Authority(pub PeerId);

/// You can filter by this component on any replicated entity to only get entities that the
/// current peer has authority over. Netvy automatically inserts this component for you.
#[derive(Component)]
pub struct Authoritative;

/// Add this plugin and specify whether this is a client or a server
/// Depending on the given `AppType`, specific systems will run
pub struct NetvyPlugin(pub NetvyMode);

/// Configure various behaviour of netvy via this resource.
/// Insert this resource before you add the NetvyPlugin, so changes are applied from the start on.
#[derive(Resource)]
pub struct NetvyConfiguration {
    /// Whether netvy should insert the bevy `Name` component into netvy entities, such as a `Client`
    /// Per default, this is on.
    add_debug_names: bool,
    /// netvy automatically despawns any clients that didn't respond in the specified
    /// `timeout_client_seconds` time. This is used to disconnect clients that didn't disconnect
    /// cleanly by triggering the `Disconnect` event. After the timeout is reached, netvy will
    /// despawn that client and any net entities that belonged to that client (both on the server
    /// and all currently connected clients).
    timeout_client_seconds: f32,
}

impl Default for NetvyConfiguration {
    fn default() -> Self {
        Self {
            add_debug_names: true,
            timeout_client_seconds: 5.0,
        }
    }
}

/// This resource tells you the current peer id in the current bevy world. On the server, this will be the PeerId of the server.
/// On a client, this will be the PeerId of the client that was spawned and used to connect with in this bevy world.
///
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
        app.add_plugins(SyncPositionPlugin);
        app.add_plugins(AliveCheckPlugin);
        app.add_plugins(DisconnectPlugin);

        match self.0 {
            NetvyMode::Client => {
                app.add_plugins(NetvyClientPlugin);
            }
            NetvyMode::Server => {
                app.add_plugins(NetvyServerPlugin);
            }
            NetvyMode::HostClient => {
                app.add_plugins(NetvyClientPlugin);
                app.add_plugins(NetvyServerPlugin);
            }
        }

        app.register_component::<Owner>();
        app.register_component::<Authority>();

        app.add_systems(
            FixedUpdate,
            (
                add_debug_name_to_clients,
                add_debug_name_to_servers,
                add_owned,
                check_invalid_net_entities,
                add_authoritative,
            ),
        );

        if cfg!(debug_assertions) {
            app.register_type::<NetEntityId>()
                .register_type::<InternalSyncPosition>()
                .register_type::<UpdateSequenceMap>()
                .register_type::<PeerId>()
                .register_type::<ConnectionState>()
                .register_type::<TargetAddress>()
                .register_type::<Owner>()
                .register_type::<Authority>();
        }
    }
}

fn get_or_create_mut_update_sequence_number(
    update_sequence: &mut UpdateSequenceMap,
    net_entity_id: NetEntityId,
    component_type_id: ComponentTypeId,
) -> &mut UpdateSequenceNumber {
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

fn add_owned(
    mut commands: Commands,
    query: Query<(Entity, &Owner), Added<Owner>>,
    our_peer_id: Option<Res<OurPeerId>>,
) {
    for (entity, owned_by) in query {
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

fn add_authoritative(
    mut commands: Commands,
    query: Query<(Entity, &Authority), Added<Authority>>,
    our_peer_id: Option<Res<OurPeerId>>,
) {
    for (entity, authority) in query {
        // NOTE: has to be in the for loop, so it only runs when Authority was added on any entity
        let Some(ref our_peer_id) = our_peer_id else {
            warn!(
                "Can't check if this entity should have Authoritative, OurPeerId resource doesn't exist yet."
            );
            continue;
        };
        if authority.0 == our_peer_id.0 {
            commands.entity(entity).insert(Authoritative);
        }
    }
}

fn check_invalid_net_entities(
    mut commands: Commands,
    query: Query<Entity, (With<SyncPosition>, Without<ReplicateEntity>)>,
) {
    for entity in query {
        warn!(
            "Entity {entity} has SyncPosition inserted, but not ReplicateEntity. This causes unexpected behaviour. netvy will now automatically insert ReplicateEntity for you."
        );
        commands.entity(entity).insert(ReplicateEntity);
    }
}
