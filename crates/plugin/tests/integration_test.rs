use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use bevy::{log::LogPlugin, prelude::*, time::TimeUpdateStrategy};
use netvy::prelude::*;
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

    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));

    app
}

fn create_server_app() -> App {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins);
    // Dont add LogPlugin because the tests run in the same process and its already added in create_client_app.
    // app.add_plugins(LogPlugin::default());
    app.add_plugins(NetvyPlugin(NetvyMode::Server));

    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));

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
    const SERVER_PORT: u16 = 5890;

    let mut server_app = create_server_app();
    let mut client_app = create_client_app();

    client_app.register_component::<TestComponent>();
    server_app.register_component::<TestComponent>();

    client_app.insert_resource(ServerPort(SERVER_PORT));
    server_app.insert_resource(ServerPort(SERVER_PORT));

    // Before we call update(), we must add all systems that should run on startup. otherwise, this
    // system will never run.
    server_app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn((TestComponent { x: 100.0 }, ReplicateEntity));
    });

    setup_server(&mut server_app);
    setup_client(&mut client_app);

    // FIXME:
    // Important: The server_app must run once first before client, so the server is started when
    // the client connects. But this shows a bug in netvy: We don't seem to retry something,
    // reproduce by just doing the client_app.update() first.
    for _ in 0..20 {
        server_app.update();
        client_app.update();
    }

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
    const SERVER_PORT: u16 = 5891;

    let mut first_client_app = create_client_app();
    let mut second_client_app = create_client_app();
    let mut server_app = create_server_app();

    first_client_app.insert_resource(ServerPort(SERVER_PORT));
    second_client_app.insert_resource(ServerPort(SERVER_PORT));
    server_app.insert_resource(ServerPort(SERVER_PORT));

    first_client_app.register_component::<TestComponent>();
    second_client_app.register_component::<TestComponent>();
    server_app.register_component::<TestComponent>();

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

#[test]
fn replicate_component_from_client_to_server() {
    const SERVER_PORT: u16 = 5892;

    let mut server_app = create_server_app();
    let mut client_app = create_client_app();

    client_app.register_component::<TestComponent>();
    server_app.register_component::<TestComponent>();

    client_app.insert_resource(ServerPort(SERVER_PORT));
    server_app.insert_resource(ServerPort(SERVER_PORT));

    // Before we call update(), we must add all systems that should run on startup. otherwise, this
    // system will never run.
    client_app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn((TestComponent { x: 100.0 }, ReplicateEntity));
    });

    setup_server(&mut server_app);
    setup_client(&mut client_app);

    // FIXME:
    // Important: The server_app must run once first before client, so the server is started when
    // the client connects. But this shows a bug in netvy: We don't seem to retry something,
    // reproduce by just doing the client_app.update() first.
    for _ in 0..20 {
        server_app.update();
        client_app.update();
    }

    let result = server_app
        .world_mut()
        .query::<&TestComponent>()
        .single(server_app.world())
        .expect("TestCompont must be replicated from server to client");

    assert_eq!(
        *result,
        TestComponent { x: 100.0 },
        "TestComponent must have correct values"
    );
}

#[test]
fn sync_position() {
    const SERVER_PORT: u16 = 5893;
    let mut client_app = create_client_app();
    let mut server_app = create_server_app();

    client_app.register_component::<Player>();
    server_app.register_component::<Player>();

    client_app.insert_resource(ServerPort(SERVER_PORT));
    server_app.insert_resource(ServerPort(SERVER_PORT));

    setup_client(&mut client_app);
    setup_server(&mut server_app);

    // client_app.add_systems(FixedUpdate, log_our_peer_id);

    server_app.add_systems(Update, spawn_player_on_client_connect);
    client_app.add_systems(Update, move_own_player);

    for _ in 0..20 {
        let mut player_on_client = client_app
            .world_mut()
            .query::<(Entity, &Player, Has<Transform>)>();
        // let count_of_players_client = player_on_client.iter(client_app.world()).len();
        // info!("count_of_players_client: {count_of_players_client}");
        for player in player_on_client.iter(client_app.world()) {
            match client_app.world().inspect_entity(player.0) {
                Ok(result) => {
                    info!(
                        "(CLIENT) Components: {:?}",
                        result.map(|comp| comp.name()).collect::<Vec<DebugName>>()
                    );
                }
                Err(error) => {
                    error!("error: {error:?}");
                }
            }
        }
        server_app.update();
        client_app.update();
    }

    let result = server_app
        .world_mut()
        .query::<(&Transform, &InternalSyncPosition)>()
        .single(server_app.world())
        .unwrap();

    let res_client = client_app
        .world_mut()
        .query::<(&Transform, &InternalSyncPosition)>()
        .single(client_app.world())
        .unwrap();

    println!("On Client: {:?}", res_client);

    println!("InternalSyncPosition on server is {:?}", result.1);
    assert_eq!(
        result.0.translation,
        vec3(5., 5., 5.),
        "Transform.translation on the server must have the correct value, coming from the authoritive client"
    );
}

// fn log_our_peer_id(our_peer_id: Option<Res<OurPeerId>>) {
//     info!("OurPeerId: {:?}", our_peer_id);
// }

#[derive(Component, Serialize, Deserialize, Debug)]
struct Player;

fn spawn_player_on_client_connect(
    mut commands: Commands,
    added_clients: Query<&PeerId, (Added<PeerId>, With<Client>)>,
) {
    for added_client in added_clients {
        info!("SPAWNED CLIENT FOR NEW PLAYER");
        commands.spawn((
            Player,
            Authority(*added_client),
            ReplicateEntity,
            SyncPosition::default(),
            Transform::default(),
        ));
    }
}

fn move_own_player(query: Query<&mut Transform, (With<Player>)>) {
    for mut added in query {
        added.translation = vec3(5., 5., 5.);
    }
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
