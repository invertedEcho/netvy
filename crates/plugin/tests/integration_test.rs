use std::net::{Ipv4Addr, SocketAddr};

use bevy::{log::LogPlugin, prelude::*};
use netvy::{SyncMode, prelude::*};
use serde::{Deserialize, Serialize};

// We store the server port in a resource, as tests run at the same time, so we need indivual server
// port for each test. And we also need access from all systems, so we just do it as a resource
#[derive(Resource)]
struct ServerPort(pub u16);

fn create_client_app() -> App {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins);
    app.add_plugins(LogPlugin::default());
    app.add_plugins(NetvyPlugin(NetvyMode::Client));

    app
}

fn create_server_app() -> App {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins);
    app.add_plugins(LogPlugin::default());
    app.add_plugins(NetvyPlugin(NetvyMode::Server));

    app
}

#[test]
fn client_connect_to_server() {
    const SERVER_PORT: u16 = 5889;
    let mut server_app = create_server_app();
    server_app.insert_resource(ServerPort(SERVER_PORT));
    setup_server(&mut server_app);

    let mut client_app = create_client_app();
    client_app.insert_resource(ServerPort(SERVER_PORT));
    setup_client(&mut client_app);

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
fn replicate_component_from_server_to_client() {
    const SERVER_PORT: u16 = 5889;

    let mut server_app = create_server_app();
    let mut client_app = create_client_app();

    client_app.register_component_with_sync_mode::<TestComponent>(SyncMode::FixedRate(0.1));
    server_app.register_component_with_sync_mode::<TestComponent>(SyncMode::FixedRate(0.1));

    client_app.insert_resource(ServerPort(SERVER_PORT));
    server_app.insert_resource(ServerPort(SERVER_PORT));

    // Before we call update(), we must add all systems that should run on startup. otherwise, this
    // system will never run.
    server_app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn((TestComponent { x: 100.0 }, ReplicateEntity));
    });

    setup_server(&mut server_app);
    setup_client(&mut client_app);

    for _ in 0..50 {
        client_app.update();
        server_app.update();
    }

    warn!(
        "OurPeer_id resource present: {:?}",
        server_app.world().get_resource::<OurPeerId>()
    );

    let result = client_app
        .world_mut()
        .query::<&TestComponent>()
        .single(client_app.world())
        .expect("TestCompont must be replicated from server to client");

    assert_eq!(
        *result,
        TestComponent { x: 100.0 },
        "TestComponent must have correct values"
    );
}

#[test]
fn replicate_component_from_client_to_client() {
    const SERVER_PORT: u16 = 5890;

    let mut first_client_app = create_client_app();
    let mut second_client_app = create_client_app();
    let mut server_app = create_server_app();

    first_client_app.register_component_with_sync_mode::<TestComponent>(SyncMode::FixedRate(0.1));
    server_app.register_component_with_sync_mode::<TestComponent>(SyncMode::FixedRate(0.1));

    setup_server(&mut server_app);
    setup_client(&mut first_client_app);
    setup_client(&mut second_client_app);

    first_client_app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn((TestComponent { x: 100.0 }, ReplicateEntity));
    });

    // spawn another client
    second_client_app.add_systems(Startup, |mut commands: Commands| {
        let socket_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), SERVER_PORT);
        let client_entity = commands.spawn((Client, TargetAddress(socket_addr))).id();
        commands.trigger(ConnectToServer { client_entity });
    });

    for _ in 0..50 {
        server_app.update();
        first_client_app.update();
        second_client_app.update();
    }

    let count_net_entities_server = server_app
        .world_mut()
        .query::<&NetEntityId>()
        .iter(server_app.world())
        .count();
    println!("Count of net entities on the server: {count_net_entities_server}");

    let count_net_entities_second_client = second_client_app
        .world_mut()
        .query::<&NetEntityId>()
        .iter(second_client_app.world())
        .count();

    println!("Count of net entities in second client app: {count_net_entities_second_client}");

    let result = second_client_app
        .world_mut()
        .query::<&TestComponent>()
        .single(second_client_app.world())
        .expect("TestCompont must be replicated from first client to second client");

    assert_eq!(
        *result,
        TestComponent { x: 100.0 },
        "TestComponent must have correct values"
    );
}

fn setup_server(server_app: &mut App) {
    server_app.add_systems(Startup, start_server);
}

fn setup_client(client_app: &mut App) {
    client_app.add_systems(Startup, spawn_client_and_connect_to_server);
}

fn start_server(mut commands: Commands, server_port: Res<ServerPort>) {
    let socket_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), server_port.0);
    let server_entity = commands.spawn((Server, TargetAddress(socket_addr))).id();

    commands.trigger(StartServer { server_entity });
}

fn spawn_client_and_connect_to_server(mut commands: Commands, server_port: Res<ServerPort>) {
    let socket_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), server_port.0);

    let client_entity = commands.spawn((Client, TargetAddress(socket_addr))).id();
    commands.trigger(ConnectToServer { client_entity });
}
