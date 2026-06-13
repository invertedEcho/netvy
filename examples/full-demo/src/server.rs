use bevy::{log::LogPlugin, prelude::*};
use bevy_inspector_egui::{bevy_egui::EguiPlugin, quick::WorldInspectorPlugin};
use netvy::prelude::*;

use crate::client::Player;

pub struct DemoServerPlugin;

impl Plugin for DemoServerPlugin {
    fn build(&self, app: &mut App) {
        let headful = if let Some(res) = std::env::args().nth(1) {
            res == "headful"
        } else {
            false
        };

        println!("Starting demo server");
        if headful {
            app.add_plugins(DefaultPlugins);
            app.add_plugins(EguiPlugin::default())
                .add_plugins(WorldInspectorPlugin::new());
        } else {
            app.add_plugins(MinimalPlugins)
                .add_plugins(LogPlugin::default());
        }

        app.add_plugins(NetvyPlugin(netvy::AppType::Server));

        app.add_systems(Startup, (start_server, spawn_camera));

        app.add_systems(Update, spawn_player_on_new_client);
    }
}

fn start_server(mut commands: Commands) {
    let server_entity = commands
        .spawn((
            Server,
            TargetAddress {
                address: "0.0.0.0".to_string(),
                port: 8080,
            },
        ))
        .id();

    commands.trigger(StartServer { server_entity });
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform {
            translation: vec3(5.0, 5.0, 5.0),
            ..default()
        }
        .looking_at(Vec3::splat(0.0), Vec3::Y),
    ));
}

/// Spawn a player whenever a new client connects.
fn spawn_player_on_new_client(mut commands: Commands, query: Query<&PeerId, Added<Client>>) {
    for peer_id in query {
        info!("Spawning a player for new client with peer id: {peer_id:?}");
        commands.spawn((
            Player,
            Transform::from_translation(vec3(0.0, 1.0, 0.0)),
            ReplicateEntity,
            SyncPosition::default(),
            Name::new("Our Player"),
            OwnedBy(*peer_id),
        ));
    }
}
