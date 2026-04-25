use bevy::prelude::*;
use bevy_multiplayer_plugin::{BevyMultiplayerFrameworkPlugin, server::StartServer};

fn main() {
    println!("Starting demo server");
    let mut app = App::new();

    app.add_plugins(DefaultPlugins);

    app.add_plugins(BevyMultiplayerFrameworkPlugin(
        bevy_multiplayer_plugin::AppType::Server,
    ));

    app.add_systems(Startup, start_server);

    app.run();
}

fn start_server(mut commands: Commands) {
    commands.trigger(StartServer { port: 8080 })
}
