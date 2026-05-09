use bevy::prelude::*;
use bevy_inspector_egui::{bevy_egui::EguiPlugin, quick::WorldInspectorPlugin};
use netvy::{NetvyPlugin, server::StartServer};

fn main() {
    println!("Starting demo server");
    let mut app = App::new();

    // app.add_plugins(MinimalPlugins)
    //     .add_plugins(LogPlugin::default());
    app.add_plugins(DefaultPlugins);

    app.add_plugins(NetvyPlugin(netvy::AppType::Server));

    app.add_plugins(EguiPlugin::default())
        .add_plugins(WorldInspectorPlugin::new());

    app.add_systems(Startup, (start_server, spawn_camera));

    app.run();
}

fn start_server(mut commands: Commands) {
    commands.trigger(StartServer { port: 8080 })
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
