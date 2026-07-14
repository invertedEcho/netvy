use std::{collections::HashMap, net::SocketAddr, time::Duration};

use bevy::{prelude::*, time::common_conditions::on_timer};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    Authority, BINCODE_CONFIG, ClientSocket, NetvyMode, OurPeerId, ServerSocket,
    component_updates::component_registry::{
        ComponentRegistry, ComponentTypeId, NextComponentTypeId,
    },
    get_or_create_mut_update_sequence_number,
    net_entity::{NetEntityId, TemporaryNetId},
    server::ConnectedClients,
    util::{DatagramType, get_byte_header_for_datagram_type, parse_u32_from_u8_arr},
};

pub mod prelude {
    pub use super::component_registry::AppComponentExt;
}

pub mod component_registry;

pub struct ComponentUpdatePlugin;

impl Plugin for ComponentUpdatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComponentUpdates>()
            .init_resource::<UpdateSequenceMap>()
            .init_resource::<FailedApplyComponentUpdates>()
            .init_resource::<ComponentRegistry>()
            .init_resource::<NextComponentTypeId>();

        app.add_systems(
            Update,
            (
                handle_failed_sent_component_updates.run_if(
                    on_timer(Duration::from_secs_f32(1.0))
                        .and(not(resource_equals(NetvyMode::HostClient))),
                ),
                handle_component_updates,
                handle_failed_apply_component_updates
                    .run_if(on_timer(Duration::from_secs_f32(1.0))),
                handle_send_interval_timers,
            ),
        );
    }
}

/// A queue for all new component updates. A system will work through this queue and apply the
/// component updates. Failed component updates will be added to the FailedApplyComponentUpdates
/// queue. We keep failed component updates seperate so we can have different logic for them.
#[derive(Resource, Default)]
pub struct ComponentUpdates(pub Vec<ComponentUpdate>);

#[derive(Debug)]
pub struct ComponentUpdate {
    net_entity_id: NetEntityId,
    component_type_id: ComponentTypeId,
    component_bytes: Vec<u8>,
    update_sequence: u32,
}

/// Stores the sequence number of component updates for each net entity and a corresponding component type id
/// Used to ensure only newer updates are applied as UDP is unordered
#[derive(Resource, Clone, Reflect, Default)]
pub struct UpdateSequenceMap(pub HashMap<(NetEntityId, ComponentTypeId), u32>);

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
    // TODO: this is kinda bad, find another way
    // if none, this FailedSentComponentUpdate stems from client, and we dont store SocketAddr of
    // server right now.
    pub target_address: Option<SocketAddr>,
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
    app_type: Res<NetvyMode>,
    connected_clients: Option<Res<ConnectedClients>>,
    server_socket: Option<Res<ServerSocket>>,
    client_socket: Option<Res<ClientSocket>>,
    our_peer_id: Option<Res<OurPeerId>>,
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
            match *app_type {
                NetvyMode::Server => {
                    for connected_client in &connected_clients {
                        failed_sent_component_updates
                            .0
                            .push(FailedSentComponentUpdate {
                                component_bytes: component_bytes.clone(),
                                component_type_id,
                                entity,
                                target_address: Some(*connected_client),
                            });
                    }
                }
                NetvyMode::Client => {
                    failed_sent_component_updates
                        .0
                        .push(FailedSentComponentUpdate {
                            component_bytes,
                            component_type_id,
                            entity,
                            target_address: None,
                        });
                }
                NetvyMode::HostClient => {
                    // this is ensured by adding a run_if condition on this system
                    unreachable!(
                        "send_component_updates_fixed_rate shouldnt run in HostClient mode"
                    );
                }
            }
            continue;
        };

        // only send changes of entities on which we have authority
        if authority.0 != our_peer_id.0 {
            continue;
        }

        let current_update_sequence = get_or_create_mut_update_sequence_number(
            &mut update_sequence,
            *net_entity_id,
            component_type_id,
        );

        *current_update_sequence += 1;

        let component_update_bytes = build_component_update_datagram(
            &component_bytes,
            component_type_id,
            net_entity_id,
            *current_update_sequence,
        );

        match *app_type {
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
                            target_address: None,
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
                                target_address: Some(*connected_client),
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
    app_type: Res<NetvyMode>,
    connected_clients: Option<Res<ConnectedClients>>,
    client_socket: Option<Res<ClientSocket>>,
    server_socket: Option<Res<ServerSocket>>,
    our_peer_id: Option<Res<OurPeerId>>,
) where
    C: Component + Serialize + DeserializeOwned,
{
    let connected_clients = connected_clients.map_or(vec![], |item| item.0.clone());

    for (entity, changed_component, maybe_net_entity, authority) in changed_entities {
        let component_type_id = component_registry.get_component_type_id::<C>();

        debug!(
            "Component changed! (entity={entity}, component_type_id={component_type_id}, maybe_net_entity={maybe_net_entity:?})"
        );

        let component_bytes =
            bincode::serde::encode_to_vec(changed_component, BINCODE_CONFIG).unwrap();

        // if the required components arent present yet, store the component update and check later
        let (Some(ref our_peer_id), Some(authority), Some(net_entity_id)) =
            (our_peer_id.as_ref(), authority, maybe_net_entity)
        else {
            match *app_type {
                NetvyMode::Server => {
                    for connected_client in &connected_clients {
                        failed_sent_component_updates
                            .0
                            .push(FailedSentComponentUpdate {
                                entity,
                                component_bytes: component_bytes.clone(),
                                component_type_id,
                                target_address: Some(*connected_client),
                            });
                    }
                }
                NetvyMode::Client => {
                    failed_sent_component_updates
                        .0
                        .push(FailedSentComponentUpdate {
                            entity,
                            component_bytes,
                            component_type_id,
                            target_address: None,
                        });
                }
                NetvyMode::HostClient => {
                    unreachable!(
                        "detect_registered_component_change shouldnt run in HostClient mode"
                    );
                }
            }
            return;
        };

        // only send changes of entities on which we have authority
        if authority.0 != our_peer_id.0 {
            debug!(
                "not sending component update for this entity, we dont have authority of this entity! (entity={entity}, net_entity_id={maybe_net_entity:?}, authority={authority:?}, our_peer_id={our_peer_id:?})"
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

        match *app_type {
            NetvyMode::Server => {
                let Some(ref socket) = server_socket else {
                    return;
                };
                let socket = &socket.0;
                for client in &connected_clients {
                    if let Err(error) = socket.send_to(&component_update, client) {
                        error!("Failed to sent component update to client {client}: {error:?}")
                    }
                }
            }
            NetvyMode::Client | NetvyMode::HostClient => {
                let Some(ref socket) = client_socket else {
                    return;
                };
                let socket = &socket.0;
                let result = socket.send(&component_update);
                if let Err(error) = result {
                    error!("Failed to sent component update: {error:?}")
                }
            }
        };
    }
}

pub fn handle_component_updates(
    mut commands: Commands,
    mut component_updates: ResMut<ComponentUpdates>,
    component_registry: Res<ComponentRegistry>,
    net_entities: Query<(Entity, Option<&TemporaryNetId>, Option<&NetEntityId>)>,
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
                error!("Failed to find apply_fn for internal_type_id: {component_type_id}");
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
        if let Some((existing_entity, _, _)) = net_entities.iter().find(|(_, _, net_entity)| {
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
                *current_update_sequence += 1;
            } else {
                // TODO: this should be moved down to the other failed_component_updates usage.
                failed_component_updates.0.push(FailedApplyComponentUpdate {
                    component_type_id,
                    net_entity_id: net_entity_id_from_component_update,
                    component_bytes,
                    incoming_update_sequence,
                });
            }
        } else {
            info!(
                "Failed to apply incoming component update, no entity with NetEntity id {} (from datagram) exists locally.",
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
             target_address,
         }| {
            let Ok(net_entity_id) = net_entities.get(*entity) else {
                debug!("Still cant sent component update, entity {entity} has no NetEntity.");
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
                    let Some(target_address) = target_address else {
                        warn!("FailedSentComponentUpdate has no target_address, but we are running on server. Deleting invalid FailedSentComponentUpdate.");
                        return false;
                    };
                    let result = socket.send_to(&data, target_address);

                    if let Err(ref error) = result {
                        warn!("Failed to send FailedSentComponentUpdate, retaining: {error}");
                    };

                    // retain if result was not ok
                    !result.is_ok()
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
        Ok(result) => Some(ComponentUpdate {
            net_entity_id: NetEntityId(bytes[1]),
            component_type_id: bytes[2],
            update_sequence: result,
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
    mut failed_component_updates: ResMut<FailedApplyComponentUpdates>,
    component_registry: Res<ComponentRegistry>,
    update_sequence: Res<UpdateSequenceMap>,
    query: Query<(Entity, &NetEntityId)>,
) {
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
