use std::net::{Ipv4Addr, SocketAddr};

use bevy::{log::LogPlugin, prelude::*};
use netvy::prelude::*;
use serde::{Deserialize, Serialize};

// We store the server port in a resource, as tests run at the same time, so we need indivual server
// port for each test. And we also need access from all systems, so we just do it as a resource
#[derive(Resource)]
struct ServerPort(pub u16);

fn create_test_client_app() -> App {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins);
    app.add_plugins(LogPlugin::default());
    app.add_plugins(NetvyPlugin(NetvyMode::Client));

    app
}

fn create_test_server_app() -> App {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins);
    app.add_plugins(LogPlugin::default());
    app.add_plugins(NetvyPlugin(NetvyMode::Server));

    app
}

#[test]
fn test_client_connect_to_server() {
    const SERVER_PORT: u16 = 5889;
    let mut server_app = create_test_server_app();
    server_app.insert_resource(ServerPort(SERVER_PORT));

    server_app.add_systems(Startup, setup_server);

    // Run once so setup_server system runs
    server_app.update();

    let mut client_app = create_test_client_app();
    client_app.insert_resource(ServerPort(SERVER_PORT));

    client_app.add_systems(Startup, spawn_client_and_connect);

    for _ in 0..50 {
        server_app.update();
        client_app.update();
    }

    let client = client_app
        .world_mut()
        .query::<(&Client, &ConnectionState)>()
        .single(client_app.world())
        .unwrap();

    assert_eq!(*client.1, ConnectionState::Connected);
}

#[derive(Component, Serialize, Deserialize, Debug, PartialEq)]
struct TestComponent {
    x: f32,
}

#[test]
fn test_replicate_entity_from_client_to_client() {
    const SERVER_PORT: u16 = 5888;
    let ClientAndServerApp {
        mut client_app,
        mut server_app,
    } = setup_client_and_server(SERVER_PORT);

    client_app.register_component::<TestComponent>();
    server_app.register_component::<TestComponent>();

    client_app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn((TestComponent { x: 100.0 }, ReplicateEntity));
    });
    client_app.update();
    client_app.update();
    client_app.update();

    // spawn another client
    let mut second_client_app = create_test_client_app();
    second_client_app.add_systems(Startup, |mut commands: Commands| {
        let socket_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), SERVER_PORT);
        let client_entity = commands.spawn((Client, TargetAddress(socket_addr))).id();
        commands.trigger(ConnectToServer { client_entity });
    });

    for _ in 0..50 {
        client_app.update();
        second_client_app.update();
        server_app.update();
    }

    let server_net_entities = second_client_app
        .world_mut()
        .query::<&NetEntityId>()
        .iter(second_client_app.world())
        .count();
    println!("Count of net entities on the server: {server_net_entities}");

    let count_net_entities = second_client_app
        .world_mut()
        .query::<&NetEntityId>()
        .iter(second_client_app.world())
        .count();

    println!("Count of net entities in second client app: {count_net_entities}");

    let result = second_client_app
        .world_mut()
        .query::<&TestComponent>()
        .single(second_client_app.world())
        .unwrap();

    assert_eq!(*result, TestComponent { x: 100.0 });
}

struct ClientAndServerApp {
    client_app: App,
    server_app: App,
}

fn setup_client_and_server(server_port: u16) -> ClientAndServerApp {
    let mut server_app = create_test_server_app();
    server_app.insert_resource(ServerPort(server_port));
    server_app.add_plugins(LogPlugin::default());

    server_app.add_systems(Startup, setup_server);

    // Run once so setup_server system runs
    server_app.update();

    let mut client_app = create_test_client_app();
    client_app.insert_resource(ServerPort(server_port));

    client_app.add_systems(Startup, spawn_client_and_connect);
    client_app.add_plugins(LogPlugin::default());

    for _ in 0..50 {
        client_app.update();
        server_app.update();
    }

    ClientAndServerApp {
        client_app,
        server_app,
    }
}

fn setup_server(mut commands: Commands, server_port: Res<ServerPort>) {
    let socket_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), server_port.0);
    let server_entity = commands.spawn((Server, TargetAddress(socket_addr))).id();

    commands.trigger(StartServer { server_entity });
}

fn spawn_client_and_connect(mut commands: Commands, server_port: Res<ServerPort>) {
    let socket_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), server_port.0);

    let client_entity = commands.spawn((Client, TargetAddress(socket_addr))).id();
    commands.trigger(ConnectToServer { client_entity });
}
