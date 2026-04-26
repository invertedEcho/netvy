use bevy::{log::LogPlugin, prelude::*};
use netvy::{NetvyPlugin, server::StartServer};

fn main() {
    println!("Starting demo server");
    let mut app = App::new();

    app.add_plugins(MinimalPlugins)
        .add_plugins(LogPlugin::default());

    app.add_plugins(NetvyPlugin(netvy::AppType::Server));

    app.add_systems(Startup, start_server);

    app.run();
}

fn start_server(mut commands: Commands) {
    commands.trigger(StartServer { port: 8080 })
}
