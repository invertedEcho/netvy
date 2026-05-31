use std::{
    any::{Any, TypeId},
    collections::HashMap,
    time::Duration,
};

use bevy::{prelude::*, time::common_conditions::on_timer};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    BINCODE_CONFIG, CurrentSocket,
    component_registry::{ComponentRegistry, ComponentTypeId},
    get_or_create_mut_update_sequence_number,
    net_entity::{NetEntity, NetEntityType, TemporaryNetId},
    util::{DatagramType, get_byte_header_for_datagram_type, parse_u32_from_u8_arr},
};

pub struct ComponentUpdatePlugin;

impl Plugin for ComponentUpdatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComponentUpdates>()
            .init_resource::<UpdateSequenceMap>()
            .init_resource::<FailedApplyComponentUpdates>();

        app.add_systems(
            Update,
            (
                handle_failed_sent_component_updates.run_if(on_timer(Duration::from_secs_f32(1.0))),
                handle_component_updates,
                handle_failed_apply_component_updates
                    .run_if(on_timer(Duration::from_secs_f32(1.0))),
                handle_send_interval_timers,
            ),
        );
    }
}

/// A queue for all new component updates. A system will work through this queue and apply the
/// component updates. Failed component updates will be retained in this queue and retried later on.
#[derive(Resource, Default)]
pub struct ComponentUpdates(pub Vec<ComponentUpdate>);

#[derive(Debug)]
pub struct ComponentUpdate {
    net_entity_id: NetEntity,
    component_type_id: ComponentTypeId,
    component_bytes: Vec<u8>,
    update_sequence: u32,
}

/// Stores the sequence number of component updates for each net entity and a corresponding component type id
/// Used to ensure only newer updates are applied as UDP is unordered
#[derive(Resource, Clone, Reflect, Default)]
pub struct UpdateSequenceMap(pub HashMap<(NetEntity, ComponentTypeId), u32>);

/// Stores component updates that failed to apply locally, for example no entity exists yet with the
/// given `net_entity_id`
#[derive(Resource, Default)]
pub struct FailedApplyComponentUpdates(pub Vec<FailedApplyComponentUpdate>);

pub struct FailedApplyComponentUpdate {
    pub component_type_id: ComponentTypeId,
    // We store the NetEntityId and not the Entity itself in case the update failed because of a
    // missing local entity (not yet spawned)
    pub net_entity_id: NetEntity,
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
    entities: Query<(Entity, &C, Option<&NetEntity>, &NetEntityType)>,
    mut update_sequence: ResMut<UpdateSequenceMap>,
    current_socket: If<Res<CurrentSocket>>,
    mut failed_sent_component_updates: ResMut<FailedSentComponentUpdates>,
) where
    C: Component + Serialize + DeserializeOwned,
{
    for (entity, component, maybe_net_entity_id, net_entity_type) in entities {
        // only send changes of our own entities
        if *net_entity_type != NetEntityType::Local {
            continue;
        }

        let type_id = TypeId::of::<C>();

        let Some(component_type_id) = component_registry
            .type_id_to_component_type_id
            .get(&type_id)
        else {
            error!("Couldnt get component type id by type id");
            return;
        };

        // we have one timer per component type id / registered component with sync mode fixed rate
        let Some(timer) = component_registry.timer.get(component_type_id) else {
            error!("Couldnt get timer for {component_type_id:?}");
            return;
        };

        let component_bytes = bincode::serde::encode_to_vec(component, BINCODE_CONFIG).unwrap();

        if !timer.is_finished() {
            return;
        };

        let Some(net_entity_id) = maybe_net_entity_id else {
            debug!(
                "Failed to get net entity id for entity {entity:?}, adding to FailedSentComponentUpdates"
            );
            failed_sent_component_updates
                .0
                .push(FailedSentComponentUpdate {
                    component_bytes,
                    component_type_id: *component_type_id,
                    entity,
                });
            return;
        };

        let current_update_sequence = get_or_create_mut_update_sequence_number(
            &mut update_sequence,
            *net_entity_id,
            *component_type_id,
        );

        *current_update_sequence += 1;

        let component_update_bytes = build_component_update_datagram(
            &component_bytes,
            *component_type_id,
            net_entity_id,
            *current_update_sequence,
        );

        // send data of changed entity / comp to server
        let result = current_socket.0.0.send(&component_update_bytes);
        debug!("Send component_update_bytes {result:?}");
    }
}

pub fn detect_registered_component_change<C>(
    component_registry: Res<ComponentRegistry>,
    changed_entities: Query<(Entity, &C, Option<&NetEntity>, Option<&NetEntityType>), Changed<C>>,
    current_socket: If<Res<CurrentSocket>>,
    mut failed_sent_component_updates: ResMut<FailedSentComponentUpdates>,
    mut update_sequence: ResMut<UpdateSequenceMap>,
) where
    C: Component + Serialize + DeserializeOwned,
{
    for (entity, changed_component, maybe_net_entity_id, maybe_net_entity_type) in changed_entities
    {
        let component_bytes =
            bincode::serde::encode_to_vec(changed_component, BINCODE_CONFIG).unwrap();

        let type_id = changed_component.type_id();

        let component_type_id = component_registry
            .type_id_to_component_type_id
            .get(&type_id)
            .expect("Given Component must be registered");

        // its possible that NetEntityType isnt present
        // -> `add_entity_type_to_sync_entities` runs after the Changed<> detection on this system
        let Some(net_entity_type) = maybe_net_entity_type else {
            failed_sent_component_updates
                .0
                .push(FailedSentComponentUpdate {
                    entity,
                    component_bytes,
                    component_type_id: *component_type_id,
                });
            continue;
        };

        if *net_entity_type != NetEntityType::Local {
            continue;
        }

        let Some(net_entity_id) = maybe_net_entity_id else {
            failed_sent_component_updates
                .0
                .push(FailedSentComponentUpdate {
                    component_bytes,
                    component_type_id: *component_type_id,
                    entity,
                });
            continue;
        };

        let current_update_sequence = get_or_create_mut_update_sequence_number(
            &mut update_sequence,
            *net_entity_id,
            *component_type_id,
        );

        *current_update_sequence += 1;

        let component_update = build_component_update_datagram(
            &component_bytes,
            *component_type_id,
            net_entity_id,
            *current_update_sequence,
        );

        let _ = current_socket.0.0.send(&component_update);
    }
}

pub fn handle_component_updates(
    mut commands: Commands,
    mut component_updates: ResMut<ComponentUpdates>,
    component_registry: Res<ComponentRegistry>,
    query: Query<(Entity, Option<&TemporaryNetId>, Option<&NetEntity>)>,
    mut update_sequence_map: ResMut<UpdateSequenceMap>,
    mut failed_component_updates: ResMut<FailedApplyComponentUpdates>,
) {
    for ComponentUpdate {
        component_type_id,
        component_bytes,
        update_sequence: incoming_update_sequence,
        net_entity_id,
    } in component_updates.0.drain(0..)
    {
        let apply_fn = {
            let Some(apply_fn) = component_registry.apply.get(&component_type_id) else {
                error!("Failed to find apply_fn for internal_type_id: {component_type_id}");
                failed_component_updates.0.push(FailedApplyComponentUpdate {
                    component_type_id,
                    net_entity_id,
                    component_bytes,
                    incoming_update_sequence,
                });
                continue;
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
                &mut update_sequence_map,
                net_entity_id,
                component_type_id,
            );

            if incoming_update_sequence <= *current_update_sequence {
                info!("Not applying update, update is older or same as current update sequence");
                continue;
            }

            let succesful = apply_fn(&mut entity_commands, &component_bytes);
            if succesful {
                *current_update_sequence += 1;
            } else {
                failed_component_updates.0.push(FailedApplyComponentUpdate {
                    component_type_id,
                    net_entity_id,
                    component_bytes,
                    incoming_update_sequence,
                });
            }
        } else {
            info!("Adding component update to FailedComponentUpdates");
            failed_component_updates.0.push(FailedApplyComponentUpdate {
                component_type_id,
                net_entity_id,
                component_bytes,
                incoming_update_sequence,
            });
        }
    }
}

fn handle_failed_sent_component_updates(
    mut resource: ResMut<FailedSentComponentUpdates>,
    entities: Query<&NetEntity>,
    current_socket: If<Res<CurrentSocket>>,
    mut update_sequence_map: ResMut<UpdateSequenceMap>,
) {
    resource.0.retain(|failed_component_update| {
        let Ok(net_entity_id) = entities.get(failed_component_update.entity) else {
            debug!("still cant apply failed component update, no matching entity");
            return true;
        };

        let current_update_sequence = get_or_create_mut_update_sequence_number(
            &mut update_sequence_map,
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
            net_entity_id: NetEntity(bytes[1]),
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
    net_entity_id: &NetEntity,
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
    query: Query<(Entity, &NetEntity)>,
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
                info!("Not applying update, update is older or same as current update sequence");
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
