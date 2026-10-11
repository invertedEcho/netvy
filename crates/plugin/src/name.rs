use bevy::prelude::*;

use crate::{NetvyConfiguration, client::Client, server::Server};

pub fn add_debug_name_to_clients(
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

pub fn add_debug_name_to_servers(
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
