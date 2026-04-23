use bevy::prelude::*;
use bevy_multiplayer_plugin::{
    AppComponentExt, BevyMultiplayerFrameworkPlugin,
    server::{ConnectToServer, StartServer},
};
use bincode::Decode;

const SERVER_PORT: u16 = 8080;

#[derive(Component, Decode)]
pub struct ExampleComponent(pub f32, pub f32);

fn main() {
    let mut app = App::new();

    app.add_plugins(DefaultPlugins);

    app.add_plugins(BevyMultiplayerFrameworkPlugin(
        bevy_multiplayer_plugin::PluginType::Client,
    ));

    app.add_systems(Startup, start_server);
    app.add_systems(Startup, start_connect.after(start_server));

    app.register_component::<ExampleComponent>();

    app.run();
}

fn start_server(mut commands: Commands) {
    commands.trigger(StartServer { port: SERVER_PORT })
}

fn start_connect(mut commands: Commands) {
    commands.trigger(ConnectToServer {
        server_url: "127.0.0.1".into(),
        port: SERVER_PORT,
    });
}
