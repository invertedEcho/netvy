use std::{collections::HashMap, time::Duration};

use bevy::{prelude::*, time::common_conditions::on_timer};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    Authority, BINCODE_CONFIG, ClientSocket, NetvyMode, OurPeerId, ServerSocket,
    component_updates::component_registry::{
        ComponentRegistry, ComponentTypeId, NextComponentTypeId,
    },
    get_or_create_mut_update_sequence_number,
    net_entity::NetEntityId,
    server::ConnectedClients,
    util::{
        DatagramType, get_byte_header_for_datagram_type, parse_u32_from_u8_arr,
        should_log_component_update,
    },
};

pub mod prelude {
    pub use super::component_registry::AppComponentExt;
}

pub mod component_registry;

pub struct ComponentUpdatePlugin;

impl Plugin for ComponentUpdatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComponentUpdatesToBeApplied>()
            .init_resource::<UpdateSequenceMap>()
            .init_resource::<FailedApplyComponentUpdates>()
            .init_resource::<ComponentRegistry>()
            .init_resource::<NextComponentTypeId>()
            .init_resource::<LatestComponentUpdates>();

        app.add_systems(
            Update,
            (
                handle_failed_sent_component_updates.run_if(
                    on_timer(Duration::from_secs_f32(1.0))
                        .and_then(not(resource_equals(NetvyMode::HostClient))),
                ),
                handle_component_updates_to_be_applied,
                handle_failed_apply_component_updates
                    .run_if(on_timer(Duration::from_secs_f32(1.0))),
                handle_send_interval_timers,
            ),
        );
    }
}

/// Stores all the latest component update, of each possible pair, e.g. NetEntityId and ComponentTypeId.
/// This is crucial, so clients connecting after component updates happened on the server will
/// receive the latest state, e.g. a snapshot.
#[derive(Resource, Default)]
pub struct LatestComponentUpdates(pub HashMap<(NetEntityId, ComponentTypeId), (Vec<u8>, u32)>);

/// A queue for all new incoming component updates that need to be applied. A system will work through this queue and apply the
/// component updates. Failed component updates will be added to the FailedApplyComponentUpdates
/// queue. We keep failed component updates seperate so we can have different logic for them.
#[derive(Resource, Default)]
pub struct ComponentUpdatesToBeApplied(pub Vec<ComponentUpdate>);

#[derive(Debug)]
pub struct ComponentUpdate {
    net_entity_id: NetEntityId,
    component_type_id: ComponentTypeId,
    component_bytes: Vec<u8>,
    update_sequence: u32,
}

pub type UpdateSequenceNumber = u32;

/// Stores the sequence number of component updates for each net entity and a corresponding component type id
/// Used to ensure only newer updates are applied as UDP is unordered
#[derive(Resource, Clone, Reflect, Default)]
pub struct UpdateSequenceMap(pub HashMap<(NetEntityId, ComponentTypeId), UpdateSequenceNumber>);

/// Stores component updates that failed to apply locally, for example no entity exists yet with the
/// given `net_entity_id`
#[derive(Resource, Default)]
pub struct FailedApplyComponentUpdates(pub Vec<FailedApplyComponentUpdate>);

pub struct FailedApplyComponentUpdate {
    pub component_type_id: ComponentTypeId,
    // We store the NetEntityId and not the Entity itself in case the update failed because of a
    // missing local entity (not yet spawned)
    pub net_entity_id: NetEntityId,
    pub component_bytes: Vec<u8>,
    pub incoming_update_sequence: u32,
}

/// Stores failed component updates that could not be sent to the server.
/// For example, an entity with a registered component changed locally, but that entity doesn't have a
/// NetEntityId yet.
#[derive(Resource, Default)]
pub struct FailedSentComponentUpdates(pub Vec<FailedSentComponentUpdate>);

pub struct FailedSentComponentUpdate {
    pub entity: Entity,
    pub component_bytes: Vec<u8>,
    pub component_type_id: u8,
}

fn handle_send_interval_timers(time: Res<Time>, mut component_registry: ResMut<ComponentRegistry>) {
    for timer in component_registry.timer.values_mut() {
        timer.tick(time.delta());
    }
}

pub fn send_component_updates_fixed_rate<C>(
    component_registry: Res<ComponentRegistry>,
    entities: Query<(Entity, &C, Option<&NetEntityId>, Option<&Authority>)>,
    mut update_sequence: ResMut<UpdateSequenceMap>,
    mut failed_sent_component_updates: ResMut<FailedSentComponentUpdates>,
    netvy_mode: Res<NetvyMode>,
    connected_clients: Option<Res<ConnectedClients>>,
    server_socket: Option<Res<ServerSocket>>,
    client_socket: Option<Res<ClientSocket>>,
    our_peer_id: Option<Res<OurPeerId>>,
    mut latest_component_updates: ResMut<LatestComponentUpdates>,
) where
    C: Component + Serialize + DeserializeOwned,
{
    let connected_clients = connected_clients.map_or(vec![], |item| item.0.clone());

    for (entity, component, maybe_net_entity_id, authority) in entities {
        let component_type_id = component_registry.get_component_type_id::<C>();

        // we have one timer per component type id / registered component with sync mode fixed rate
        let Some(timer) = component_registry.timer.get(&component_type_id) else {
            error!("Couldnt get timer for {component_type_id:?}");
            return;
        };

        if !timer.is_finished() {
            return;
        };

        let component_bytes = bincode::serde::encode_to_vec(component, BINCODE_CONFIG).unwrap();

        // If the entity doesnt have the authority present yet, we must store it, so we can retry
        // later. Once the Authority component is present, we either:
        //   - remove the component update and do nothing if it turns out we dont have authority over
        //     this component
        //   - send the component update if it turns out that we actually have authority
        let (Some(authority), Some(our_peer_id), Some(net_entity_id)) =
            (authority, our_peer_id.as_ref(), maybe_net_entity_id)
        else {
            failed_sent_component_updates
                .0
                .push(FailedSentComponentUpdate {
                    component_bytes: component_bytes.clone(),
                    component_type_id,
                    entity,
                });
            continue;
        };

        // dont need to check NetvyMode::HostClient as this system wont run in this case
        let is_server = *netvy_mode == NetvyMode::Server;

        if authority.0 != our_peer_id.0 || !is_server {
            continue;
        }

        let current_update_sequence = get_or_create_mut_update_sequence_number(
            &mut update_sequence,
            *net_entity_id,
            component_type_id,
        );

        // Every time a component changes/fixed rate, we increase
        *current_update_sequence += 1;

        let component_update_bytes = build_component_update_datagram(
            &component_bytes,
            component_type_id,
            net_entity_id,
            *current_update_sequence,
        );

        if should_log_component_update(component_type_id) {
            debug!(
                "Added a component update to latest_component_updates component_type_id={component_type_id}"
            );
        }

        latest_component_updates.0.insert(
            (*net_entity_id, component_type_id),
            (component_bytes.clone(), *current_update_sequence),
        );

        match *netvy_mode {
            NetvyMode::Client => {
                let Some(ref client_socket) = client_socket else {
                    warn!("Cant send component update, no ClientSocket exists");
                    continue;
                };
                let result = client_socket.0.send(&component_update_bytes);
                if let Err(error) = result {
                    error!(
                        "Failed to send ComponentUpdate, adding to FailedSentComponentUpdates: {}",
                        error
                    );
                    failed_sent_component_updates
                        .0
                        .push(FailedSentComponentUpdate {
                            entity,
                            component_bytes,
                            component_type_id,
                        })
                }
            }
            NetvyMode::Server => {
                let Some(ref server_socket) = server_socket else {
                    warn!("Cant send component update, no ServerSocket exists");
                    continue;
                };
                for connected_client in &connected_clients {
                    let result = server_socket
                        .0
                        .send_to(&component_update_bytes, connected_client);
                    if let Err(error) = result {
                        error!(
                            "Failed to send ComponentUpdate, adding to FailedSentComponentUpdates: {}",
                            error
                        );
                        failed_sent_component_updates
                            .0
                            .push(FailedSentComponentUpdate {
                                entity,
                                component_bytes: component_bytes.clone(),
                                component_type_id,
                            })
                    }
                }
            }
            NetvyMode::HostClient => {
                unreachable!("send_component_updates_fixed_rate shouldnt run in HostClient mode");
            }
        }
    }
}

pub fn detect_registered_component_change<C>(
    component_registry: Res<ComponentRegistry>,
    changed_entities: Query<(Entity, &C, Option<&NetEntityId>, Option<&Authority>), Changed<C>>,
    mut failed_sent_component_updates: ResMut<FailedSentComponentUpdates>,
    mut update_sequence: ResMut<UpdateSequenceMap>,
    netvy_mode: Res<NetvyMode>,
    connected_clients: Option<Res<ConnectedClients>>,
    client_socket: Option<Res<ClientSocket>>,
    server_socket: Option<Res<ServerSocket>>,
    our_peer_id: Option<Res<OurPeerId>>,
    mut latest_component_updates: ResMut<LatestComponentUpdates>,
) where
    C: Component + Serialize + DeserializeOwned,
{
    let connected_clients = connected_clients.map_or(vec![], |item| item.0.clone());
    let component_type_id = component_registry.get_component_type_id::<C>();

    for (entity, changed_component, maybe_net_entity, authority) in changed_entities {
        let component_bytes =
            bincode::serde::encode_to_vec(changed_component, BINCODE_CONFIG).unwrap();

        // if the required components arent present yet, store the component update and check later
        let (Some(ref our_peer_id), Some(authority), Some(net_entity_id)) =
            (our_peer_id.as_ref(), authority, maybe_net_entity)
        else {
            if should_log_component_update(component_type_id) {
                debug!(
                    ?entity,
                    ?component_type_id,
                    ?our_peer_id,
                    ?authority,
                    ?maybe_net_entity,
                    ?netvy_mode,
                    "Failed to sent component update: Some required components are not yet present. Adding to queue to handle later"
                );
            }
            failed_sent_component_updates
                .0
                .push(FailedSentComponentUpdate {
                    entity,
                    component_bytes: component_bytes.clone(),
                    component_type_id,
                });
            return;
        };

        // dont send the component update if this is the latest component update. this is crucial,
        // as we would otherwise detect a component update that came from netvy, e.g. it was applied from
        // received component update.
        if let Some(latest_component_update) = latest_component_updates
            .0
            .get(&(*net_entity_id, component_type_id))
        {
            if latest_component_update.0 == component_bytes {
                return;
            }
        }

        // dont need to check NetvyMode::HostClient as this system wont run in this case
        let is_server = *netvy_mode == NetvyMode::Server;

        let we_have_authority = authority.0.0 == our_peer_id.0.0;

        if !we_have_authority && !is_server {
            debug!(
                ?authority,
                ?our_peer_id,
                "Registered component changed but we neither have authority nor are we the server, skipping"
            );
            continue;
        }

        let current_update_sequence = get_or_create_mut_update_sequence_number(
            &mut update_sequence,
            *net_entity_id,
            component_type_id,
        );

        *current_update_sequence += 1;

        let component_update = build_component_update_datagram(
            &component_bytes,
            component_type_id,
            net_entity_id,
            *current_update_sequence,
        );

        latest_component_updates.0.insert(
            (*net_entity_id, component_type_id),
            (component_bytes.clone(), *current_update_sequence),
        );

        if should_log_component_update(component_type_id) {
            debug!(
                ?component_type_id,
                "Component changed, added it to LatestComponentUpdates"
            );
        }

        match *netvy_mode {
            NetvyMode::Server => {
                let Some(ref socket) = server_socket else {
                    error!(
                        "Failed to sent component update: Running in NetvyMode::Server, but ServerSocket doesn't exist."
                    );
                    return;
                };
                let socket = &socket.0;
                for client in &connected_clients {
                    if let Err(error) = socket.send_to(&component_update, client) {
                        error!("Failed to sent component update to client {client}: {error:?}")
                    } else {
                        if should_log_component_update(component_type_id) {
                            debug!(
                                ?component_type_id,
                                ?entity,
                                ?maybe_net_entity,
                                ?client,
                                "Succesfully sent component update to client"
                            )
                        }
                    }
                }
            }
            NetvyMode::Client | NetvyMode::HostClient => {
                let Some(ref socket) = client_socket else {
                    error!(
                        "Failed to sent component update: Running in {netvy_mode:?}, but ClientSocket doesn't exist."
                    );
                    return;
                };
                let socket = &socket.0;
                let result = socket.send(&component_update);
                if let Err(error) = result {
                    error!("Failed to sent component update: {error:?}")
                } else {
                    if should_log_component_update(component_type_id) {
                        debug!(
                            ?component_type_id,
                            ?entity,
                            ?maybe_net_entity,
                            "Succesfully sent component update to server"
                        )
                    }
                }
            }
        };
    }
}

pub fn handle_component_updates_to_be_applied(
    mut commands: Commands,
    mut component_updates: ResMut<ComponentUpdatesToBeApplied>,
    component_registry: Res<ComponentRegistry>,
    net_entities: Query<(Entity, Option<&NetEntityId>)>,
    mut update_sequence_map: ResMut<UpdateSequenceMap>,
    mut failed_component_updates: ResMut<FailedApplyComponentUpdates>,
) {
    for ComponentUpdate {
        component_type_id,
        component_bytes,
        update_sequence: incoming_update_sequence,
        net_entity_id: net_entity_id_from_component_update,
    } in component_updates.0.drain(0..)
    {
        let apply_fn = {
            let Some(apply_fn) = component_registry.apply.get(&component_type_id) else {
                error!(
                    "Cant apply component update: Failed to find apply_fn for internal_type_id: {component_type_id}"
                );
                failed_component_updates.0.push(FailedApplyComponentUpdate {
                    component_type_id,
                    net_entity_id: net_entity_id_from_component_update,
                    component_bytes,
                    incoming_update_sequence,
                });
                continue;
            };
            *apply_fn
        };

        // try to find the local entity, matching against the net entity id from the component update
        if let Some((existing_entity, _)) = net_entities.iter().find(|(_, net_entity)| {
            let Some(net_entity) = net_entity else {
                return false;
            };
            net_entity.0 == net_entity_id_from_component_update.0
        }) {
            let mut entity_commands = commands.entity(existing_entity);

            let current_update_sequence = get_or_create_mut_update_sequence_number(
                &mut update_sequence_map,
                net_entity_id_from_component_update,
                component_type_id,
            );

            if incoming_update_sequence <= *current_update_sequence {
                debug!("Not applying update, update is older or same as current update sequence");
                continue;
            }

            let succesful = apply_fn(&mut entity_commands, &component_bytes);
            if succesful {
                if should_log_component_update(component_type_id) {
                    debug!(?component_type_id, "Succesfully applied component update");
                }
                *current_update_sequence += 1;
            } else {
                debug!("Failed to apply component update (component_type_id={component_type_id})");
                failed_component_updates.0.push(FailedApplyComponentUpdate {
                    component_type_id,
                    net_entity_id: net_entity_id_from_component_update,
                    component_bytes,
                    incoming_update_sequence,
                });
            }
        } else {
            info!(
                "Failed to apply incoming component update, no entity with NetEntity id {} (from datagram) exists locally, adding to FailedApplyComponentUpdate",
                net_entity_id_from_component_update.0
            );
            failed_component_updates.0.push(FailedApplyComponentUpdate {
                component_type_id,
                net_entity_id: net_entity_id_from_component_update,
                component_bytes,
                incoming_update_sequence,
            });
        }
    }
}

fn handle_failed_sent_component_updates(
    mut resource: ResMut<FailedSentComponentUpdates>,
    net_entities: Query<&NetEntityId>,
    mut update_sequence_map: ResMut<UpdateSequenceMap>,
    app_type: Res<NetvyMode>,
    client_socket: Option<Res<ClientSocket>>,
    server_socket: Option<Res<ServerSocket>>,
    mut latest_component_updates: ResMut<LatestComponentUpdates>,
    connected_clients: Option<Res<ConnectedClients>>,
) {
    if resource.0.is_empty() {
        return;
    }

    let socket = match *app_type {
        NetvyMode::Server => {
            let Some(ref socket) = server_socket else {
                return;
            };
            &socket.0
        }
        NetvyMode::Client | NetvyMode::HostClient => {
            let Some(ref socket) = client_socket else {
                return;
            };
            &socket.0
        }
    };

    resource.0.retain(
        |FailedSentComponentUpdate {
             entity,
             component_bytes,
             component_type_id,
         }| {
            if should_log_component_update(*component_type_id) {
                debug!(?entity, ?component_type_id, "Handling a FailedSentComponentUpdate");
            }
            let Ok(net_entity_id) = net_entities.get(*entity) else {
                debug!(entity = ?entity, "Still cant sent component update, entity has no NetEntity.");
                return true;
            };

            let current_update_sequence = get_or_create_mut_update_sequence_number(
                &mut update_sequence_map,
                *net_entity_id,
                *component_type_id,
            );

            *current_update_sequence += 1;

            // TODO: did we already build the datagram where we add failed sent component udpates?
            // then we could save us this work
            let data = build_component_update_datagram(
                component_bytes,
                *component_type_id,
                net_entity_id,
                *current_update_sequence,
            );

            if should_log_component_update(*component_type_id) {
                debug!(
                    ?component_type_id,
                    "Added a component update to latest_component_updates"
                );
            }

            latest_component_updates.0.insert(
                (*net_entity_id, *component_type_id),
                (component_bytes.clone(), *current_update_sequence),
            );

            match *app_type {
                NetvyMode::Client => {
                    // dont retain if sending was succesful
                    let result = socket.send(&data);
                    if let Err(ref error) = result {
                        warn!("Failed to send FailedSentComponentUpdate, retaining: {error}");
                    };

                    // retain if result was not ok
                    !result.is_ok()
                }
                NetvyMode::Server => {
                    let connected_clients = connected_clients.as_ref().expect("ConnectedClients resource must be initialized when running with NetvyMode::Server");

                    // FIXME: This is kinda dirty. As soon as we failed to send the component update
                    // to one client, we retain the component update, which will mean the component
                    // update may be sent to a client more than once, e.g. to the clients netvy was
                    // able to sent this component update succesfully. But it kinda doesnt matter
                    // because we check update sequence number anyways. But eventually we want to
                    // fix this because we could save bandwidth.
                    let mut any_not_ok = false;

                    for client in &connected_clients.0 {
                        let result = socket.send_to(&data, client);

                        if let Err(ref error) = result {
                            warn!("Failed to send FailedSentComponentUpdate, retaining: {error}");
                            any_not_ok = true;
                        } else {
                            debug!(?client, ?component_type_id, "Succesfully sent previously failed to send component update to client");
                        };

                    }
                    any_not_ok
                }
                NetvyMode::HostClient => {
                    unreachable!("handle_failed_sent_component_updates shouldnt run in HostClient mode");
                }
            }
        },
    );
}

pub fn get_component_update_from_datagram(bytes: &[u8]) -> Option<ComponentUpdate> {
    if bytes[0] != get_byte_header_for_datagram_type(DatagramType::ComponentUpdate) {
        return None;
    }

    // if bytes.len() < 8 {
    //     warn!("bytes are too short to be a ComponentUpdate. {bytes:?}");
    //     return None;
    // }

    match parse_u32_from_u8_arr(bytes, 3, 7) {
        Ok(update_sequence_number) => Some(ComponentUpdate {
            net_entity_id: NetEntityId(bytes[1]),
            component_type_id: bytes[2],
            update_sequence: update_sequence_number,
            component_bytes: bytes[7..].into(),
        }),
        Err(error) => {
            error!(
                "Failed to get sequence update bytes from component update datagram. bytes: {bytes:?}\n{error:?}"
            );
            None
        }
    }
}

pub fn build_component_update_datagram(
    component_bytes: &[u8],
    component_type_id: u8,
    net_entity_id: &NetEntityId,
    current_update_sequence: u32,
) -> Vec<u8> {
    let mut data = Vec::new();

    data.extend_from_slice(&[get_byte_header_for_datagram_type(
        DatagramType::ComponentUpdate,
    )]);

    data.extend_from_slice(&[net_entity_id.0]);

    data.extend_from_slice(&[component_type_id]);

    let new_update_sequence = current_update_sequence.to_be_bytes();

    data.extend_from_slice(&new_update_sequence);

    data.extend_from_slice(component_bytes);
    data
}

pub fn handle_failed_apply_component_updates(
    mut commands: Commands,
    mut failed_apply_component_updates: ResMut<FailedApplyComponentUpdates>,
    component_registry: Res<ComponentRegistry>,
    mut update_sequence: ResMut<UpdateSequenceMap>,
    query: Query<(Entity, &NetEntityId)>,
) {
    failed_apply_component_updates
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

            let current_update_sequence = get_or_create_mut_update_sequence_number(
                &mut update_sequence,
                *net_entity_id,
                *component_type_id,
            );

            if failed_component_update.incoming_update_sequence <= *current_update_sequence {
                debug!("Not applying update, update is older or same as current update sequence");
                return false;
            }

            let mut entity_commands = commands.entity(entity);

            apply_fn(
                &mut entity_commands,
                &failed_component_update.component_bytes,
            );
            false
        });
}
