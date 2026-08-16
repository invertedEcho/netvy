use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use bevy::{log::LogPlugin, prelude::*, time::TimeUpdateStrategy};
use netvy::prelude::*;

// We store the server port in a resource, as tests run at the same time, so we need indivual server
// port for each test. And we also need access from all systems, so we just do it as a resource
#[derive(Resource)]
pub struct ServerPort(pub u16);

pub fn create_client_app() -> App {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins);
    app.add_plugins(LogPlugin::default());
    app.add_plugins(NetvyPlugin(NetvyMode::Client));

    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));

    app
}

pub fn create_server_app() -> App {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins);
    // Dont add LogPlugin because the tests run in the same process and its already added in create_client_app.
    // app.add_plugins(LogPlugin::default());
    app.add_plugins(NetvyPlugin(NetvyMode::Server));

    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));

    app
}

pub fn start_server(mut commands: Commands, server_port: Res<ServerPort>) {
    let socket_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), server_port.0);
    let server_entity = commands.spawn((Server, TargetAddress(socket_addr))).id();

    commands.trigger(StartServer { server_entity });
}

pub fn spawn_client_and_connect_to_server(mut commands: Commands, server_port: Res<ServerPort>) {
    let socket_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), server_port.0);

    let client_entity = commands.spawn((Client, TargetAddress(socket_addr))).id();
    commands.trigger(ConnectToServer { client_entity });
}
