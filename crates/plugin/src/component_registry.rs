use crate::component_updates::{
    detect_registered_component_change, send_component_updates_fixed_rate,
};
use std::{any::TypeId, collections::HashMap};

use bevy::prelude::*;
use bincode::error::DecodeError;
use serde::{Serialize, de::DeserializeOwned};

use crate::{BINCODE_CONFIG, SyncMode};

// returns whether applying the update was succesful
type ApplyFn = fn(&mut EntityCommands, &[u8]) -> bool;

// We cant use bevys component id, because they are not stable across worlds.
// This is what gets sent in the datagram, and then we can lookup the corresponding
// deserialize fn in the `ComponentRegistry`

#[derive(Resource, Default)]
pub struct NextComponentTypeId(pub ComponentTypeId);

pub type ComponentTypeId = u8;

// while this allows us to create a mapping for new registered components, if we now actually want
// to know the ComponentTypeId for a type<C>, that wont work. so we also need to store that
// information. we do so by using rusts TypeId. even if this is not stable, it doesnt matter because
// each client has this mapping
// TODO: this is not completely stable. we would need deterministic ID so this is less likely to break
#[derive(Resource, Default)]
pub struct ComponentRegistry {
    pub apply: HashMap<ComponentTypeId, ApplyFn>,
    pub type_id_to_component_type_id: HashMap<TypeId, ComponentTypeId>,
    pub timer: HashMap<ComponentTypeId, Timer>,
}

pub trait AppComponentExt {
    /// Register a component in order for it to be replicated and synced across clients and servers.
    /// This uses the default SyncMode.
    fn register_component<C>(&mut self)
    where
        C: Component + Serialize + DeserializeOwned;

    /// If you want to specify how frequent updates should be done for the specified component, you
    /// may do so by using the paramter `sync_mode`
    fn register_component_with_sync_mode<C>(&mut self, sync_mode: SyncMode)
    where
        C: Component + Serialize + DeserializeOwned;
}

impl AppComponentExt for App {
    fn register_component<C>(&mut self)
    where
        C: Component + Serialize + DeserializeOwned,
    {
        self.register_component_with_sync_mode::<C>(SyncMode::default());
    }

    fn register_component_with_sync_mode<C>(&mut self, sync_mode: SyncMode)
    where
        C: Component + Serialize + DeserializeOwned,
    {
        let world = self.world_mut();

        let component_type_id = {
            let Some(mut next) = world.get_resource_mut::<NextComponentTypeId>() else {
                panic!("Please ensure NetvyPlugin is added before calling register_component().");
            };
            let id = next.0;
            next.0 += 1;
            id
        };

        let mut component_registry = world.resource_mut::<ComponentRegistry>();

        component_registry
            .apply
            .insert(component_type_id, |entity_commands, bytes| {
                let Ok((component, _size)): Result<(C, usize), DecodeError> =
                    bincode::serde::decode_from_slice(bytes, BINCODE_CONFIG)
                else {
                    warn!("Couldnt decode bytes");
                    return false;
                };

                entity_commands.insert(component);
                true
            });

        component_registry
            .type_id_to_component_type_id
            .insert(TypeId::of::<C>(), component_type_id);

        match sync_mode {
            SyncMode::FixedRate(fixed_rate) => {
                component_registry.timer.insert(
                    component_type_id,
                    Timer::from_seconds(fixed_rate, TimerMode::Repeating),
                );
                self.add_systems(Update, send_component_updates_fixed_rate::<C>);
            }
            SyncMode::OnChange => {
                self.add_systems(Update, detect_registered_component_change::<C>);
            }
        }

        info!(
            "Registered a new component! {}. component_type_id: \
             {component_type_id}",
            std::any::type_name::<C>()
        );
    }
}
