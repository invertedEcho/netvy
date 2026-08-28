use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    ClientSocket, NetvyMode, OurPeerId, Owner, PeerId,
    alive_check::AliveChecks,
    net_entity::NetEntityId,
    network_messages::{
        AppNetworkMessageExt, FromClient, FromServer, MessageDirection, NetworkMessageTarget,
        ToClients, ToServer,
    },
    server::{ConnectedClients, SocketAddrToPeerId},
    utils::reverse_hash_map_lookup,
};

pub mod prelude {
    pub use crate::disconnect::{ClientDisconnected, ClientDisconnectedServer, Disconnect};
}

pub struct DisconnectPlugin;

impl Plugin for DisconnectPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ClientDisconnectedServer>();

        app.register_network_message::<DespawnNetEntities>(MessageDirection::ServerToClients);
        app.register_network_message::<ClientDisconnected>(MessageDirection::ServerToClients);
        app.register_network_message::<InternalDisconnectMessage>(MessageDirection::ClientToServer);
        app.register_network_message::<ConfirmDisconnect>(MessageDirection::ServerToClient);

        app.add_systems(
            FixedUpdate,
            (
                read_despawn_net_entities_messages,
                read_confirm_disconnect,
                handle_internal_disconnect_message.run_if(
                    resource_equals(NetvyMode::Server)
                        .or_else(resource_equals(NetvyMode::HostClient)),
                ),
                handle_client_disconnected_message,
            ),
        );

        app.add_observer(handle_disconnect_event);
    }
}

/// Trigger this event on the client to disconnect from the server. The client entity will be
/// despawned from all connected peers alongside all net entities belonging to that client.
#[derive(Event)]
pub struct Disconnect;

// internal network message, sent from server to clients to despawn all net entities of
// disconnected/timed out client.
#[derive(Message, Serialize, Deserialize)]
pub struct DespawnNetEntities(pub Vec<NetEntityId>);

/// A client can trigger the `Disconnect` event. netvy will handle this event and send this
/// `InternalDisconnectMessage` network message to the server.
#[derive(Message, Serialize, Deserialize)]
struct InternalDisconnectMessage;

/// This message can be used to be informed whenever a client has disconnected. It is sent from the
/// server to all connected clients. Read it on the client.
#[derive(Message, Serialize, Deserialize)]
pub struct ClientDisconnected {
    pub client: PeerId,
}

/// This message can be used to be informed on the server whenever a client has disconnected. Read
/// it on the server.
// As this is written on the server, and only be supposed to be read from the server, it doesnt need
// to be a network message.
#[derive(Message)]
pub struct ClientDisconnectedServer {
    pub client: PeerId,
}

/// A server can send this network message to a client that "requested" a disconnect. The server
/// confirms the reception of the initial disconnect message from the client with this network message.
/// The client will despawn any net entities and resources from netvy upon receiving this network message.
#[derive(Message, Serialize, Deserialize)]
pub struct ConfirmDisconnect;

fn read_despawn_net_entities_messages(
    mut commands: Commands,
    mut message_reader: MessageReader<FromServer<DespawnNetEntities>>,
    net_entities: Query<(Entity, &NetEntityId)>,
) {
    for message in message_reader.read() {
        for net_entity_to_despawn in &message.0.0 {
            let Some(entity) = net_entities
                .iter()
                .find(|(_, net_entity)| net_entity.0 == net_entity_to_despawn.0)
                .map(|(entity, _)| entity)
            else {
                error!(
                    ?net_entity_to_despawn,
                    "Failed to despawn entity from DespawnNetEntities message: No entity exists with the given NetEntityId"
                );
                continue;
            };

            commands.entity(entity).despawn();
        }
    }
}

fn handle_disconnect_event(
    _: On<Disconnect>,
    mut commands: Commands,
    peer_query: Query<Entity, With<PeerId>>,
    net_entities: Query<Entity, With<NetEntityId>>,
    mut message_writer: MessageWriter<ToServer<InternalDisconnectMessage>>,
) {
    // we can already despawn any entities from netvy.
    for net_entity in net_entities {
        commands.entity(net_entity).despawn();
    }

    for peer_entity in peer_query {
        commands.entity(peer_entity).despawn();
    }

    // we dont drop the client socket and all of netvy resources yet as we still need it to send this message.
    // only when server responded with ConfirmDisconnect, we do that.
    message_writer.write(ToServer(InternalDisconnectMessage));
}

fn read_confirm_disconnect(
    mut message_reader: MessageReader<FromServer<ConfirmDisconnect>>,
    mut commands: Commands,
) {
    for _ in message_reader.read() {
        info!("Received ConfirmDisconnect message from server, removing all resources of netvy");
        commands.remove_resource::<ClientSocket>();
        commands.remove_resource::<OurPeerId>();
    }
}

fn handle_internal_disconnect_message(
    mut message_reader: MessageReader<FromClient<InternalDisconnectMessage>>,
    mut commands: Commands,
    client_query: Query<(Entity, &PeerId)>,
    net_entities: Query<(Entity, &Owner, &NetEntityId)>,
    mut despawn_net_entities_message_writer: MessageWriter<ToClients<DespawnNetEntities>>,
    mut client_disconnect_message_writer: MessageWriter<ToClients<ClientDisconnected>>,
    mut alive_checks: ResMut<AliveChecks>,
    mut connected_clients: ResMut<ConnectedClients>,
    socket_addr_to_peer_id: Res<SocketAddrToPeerId>,
    mut client_disconnected_server_message_writer: MessageWriter<ClientDisconnectedServer>,
    mut confirm_disconnect_message_writer: MessageWriter<ToClients<ConfirmDisconnect>>,
) {
    for message in message_reader.read() {
        let peer_id_of_client_disconnecting = message.source_client;
        let socket_addr =
            reverse_hash_map_lookup(&socket_addr_to_peer_id.0, peer_id_of_client_disconnecting)
                .expect("Invariant violation: A PeerId must always have a SocketAddr.");
        let index = connected_clients.0.iter().position(|s| *s == socket_addr).expect("Invariant violation: A SocketAddr contained in SocketAddrToPeerId must also be contained in ConnectedClients.");

        connected_clients.0.swap_remove(index);

        alive_checks.0.remove(&peer_id_of_client_disconnecting);

        if let Some(client_entity) = client_query
            .iter()
            .find(|(_, peer_id)| peer_id.0 == peer_id_of_client_disconnecting.0)
            .map(|(entity, _)| entity)
        {
            commands.entity(client_entity).despawn();
        }
        let mut net_entities_to_despawn: Vec<NetEntityId> = vec![];
        for (entity, owner, net_entity_id) in net_entities {
            if owner.0.0 == peer_id_of_client_disconnecting.0 {
                commands.entity(entity).despawn();
                net_entities_to_despawn.push(*net_entity_id);
            }
        }
        if !net_entities_to_despawn.is_empty() {
            let message = DespawnNetEntities(net_entities_to_despawn);
            despawn_net_entities_message_writer.write(ToClients {
                message,
                target: NetworkMessageTarget::All,
            });
        }

        let message = ClientDisconnected {
            client: peer_id_of_client_disconnecting,
        };

        client_disconnect_message_writer.write(ToClients {
            message,
            target: NetworkMessageTarget::All,
        });
        client_disconnected_server_message_writer.write(ClientDisconnectedServer {
            client: peer_id_of_client_disconnecting,
        });

        confirm_disconnect_message_writer.write(ToClients {
            message: ConfirmDisconnect,
            target: NetworkMessageTarget::Single(peer_id_of_client_disconnecting),
        });
    }
}

fn handle_client_disconnected_message(
    mut message_reader: MessageReader<FromServer<ClientDisconnected>>,
    our_peer_id: Option<Res<OurPeerId>>,
    mut commands: Commands,
) {
    for message in message_reader.read() {
        let Some(ref our_peer_id) = our_peer_id else {
            error!(
                "Received ClientDisconnected message but OurPeerId doesnt exist, cant determine whether to close our UDP socket"
            );
            continue;
        };
        if message.0.client.0 == our_peer_id.0.0 {
            // is removing the resource enough? it should drop the udp socket, effectively closing it?
            commands.remove_resource::<ClientSocket>();
        }
    }
}
