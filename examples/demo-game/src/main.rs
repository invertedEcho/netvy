use bevy::prelude::*;
use bevy_multiplayer_plugin::{
    AppComponentExt, BevyMultiplayerFrameworkPlugin,
    server::{ConnectToServer, StartServer},
};
use bincode::{Decode, Encode};

const SERVER_PORT: u16 = 8080;

#[derive(Component, Decode, Encode)]
pub struct ExampleComponent(pub f32, pub f32);

fn main() {
    let mut app = App::new();

    app.add_plugins(DefaultPlugins);

    app.add_plugins(BevyMultiplayerFrameworkPlugin(
        bevy_multiplayer_plugin::PluginType::Client,
    ));

    app.add_systems(Startup, start_server);
    app.add_systems(Startup, start_connect.after(start_server));

    app.add_systems(Update, change_registered_component);

    app.register_component::<ExampleComponent>();

    app.run();
}

fn start_server(mut commands: Commands) {
    commands.spawn(ExampleComponent(0.0, 0.0));

    commands.trigger(StartServer { port: SERVER_PORT })
}

fn start_connect(mut commands: Commands) {
    commands.trigger(ConnectToServer {
        server_url: "127.0.0.1".into(),
        port: SERVER_PORT,
    });
}

fn change_registered_component(mut single_c: Single<&mut ExampleComponent>) {
    single_c.0 += 1.0;
}
