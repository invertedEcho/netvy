use bevy::prelude::*;
use netvy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::common::{
    ServerPort, create_client_app, create_server_app, spawn_client_and_connect_to_server,
    start_server,
};

mod common;

#[test]
fn client_connect_to_server() {
    const SERVER_PORT: u16 = 5889;
    let mut server_app = create_server_app();
    server_app.insert_resource(ServerPort(SERVER_PORT));
    server_app.add_systems(Startup, start_server);

    let mut client_app = create_client_app();
    client_app.insert_resource(ServerPort(SERVER_PORT));
    client_app.add_systems(Startup, spawn_client_and_connect_to_server);

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

    server_app.add_systems(Startup, start_server);

    server_app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn((TestComponent { x: 100.0 }, ReplicateEntity));
    });

    client_app.add_systems(Startup, spawn_client_and_connect_to_server);

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

    server_app.add_systems(Startup, start_server);
    first_client_app.add_systems(Startup, spawn_client_and_connect_to_server);
    second_client_app.add_systems(Startup, spawn_client_and_connect_to_server);

    first_client_app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn((TestComponent { x: 100.0 }, ReplicateEntity));
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

    server_app.add_systems(Startup, start_server);
    client_app.add_systems(Startup, spawn_client_and_connect_to_server);

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

    server_app.add_systems(Startup, start_server);
    client_app.add_systems(Startup, spawn_client_and_connect_to_server);

    server_app.add_systems(Update, spawn_player_on_client_connect);
    client_app.add_systems(Update, move_own_player);

    for _ in 0..20 {
        server_app.update();
        client_app.update();
    }

    let transform_on_server = server_app
        .world_mut()
        .query::<&Transform>()
        .single(server_app.world())
        .unwrap();

    assert_eq!(
        transform_on_server.translation,
        vec3(5., 5., 5.),
        "Transform.translation on the server must have the correct value, coming from the authoritive client"
    );
}

#[derive(Component, Serialize, Deserialize, Debug)]
struct Player;

fn spawn_player_on_client_connect(
    mut commands: Commands,
    added_clients: Query<&PeerId, (Added<PeerId>, With<Client>)>,
) {
    for added_client in added_clients {
        info!("Spawned a player for new connected client");
        commands.spawn((
            Player,
            Authority(*added_client),
            ReplicateEntity,
            SyncPosition::default(),
            Transform::default(),
        ));
    }
}

fn move_own_player(query: Query<&mut Transform, With<Player>>) {
    for mut added in query {
        added.translation = vec3(5., 5., 5.);
    }
}

/// Check whether triggering a Disconnect on the client despawns the client entity on the server.
#[test]
fn trigger_disconnect() {
    const SERVER_PORT: u16 = 5894;

    let mut server_app = create_server_app();
    let mut client_app = create_client_app();

    server_app.insert_resource(ServerPort(SERVER_PORT));
    client_app.insert_resource(ServerPort(SERVER_PORT));

    server_app.add_systems(Startup, start_server);
    client_app.add_systems(Startup, spawn_client_and_connect_to_server);

    for _ in 0..20 {
        server_app.update();
        client_app.update();
    }

    let mut client_query = server_app.world_mut().query::<&Client>();
    let count_of_clients = client_query.iter(server_app.world()).len();
    assert_eq!(count_of_clients, 1);

    client_app.add_systems(Update, disconnect);

    for _ in 0..20 {
        server_app.update();
        client_app.update();
    }

    let mut client_query_server = server_app.world_mut().query::<&Client>();
    let count_of_clients_server = client_query_server.iter(server_app.world()).len();

    assert_eq!(
        count_of_clients_server, 0,
        "After triggering a disconnect on the client, the client entity must not exist anymore on the server"
    );

    let mut client_query_client = client_app.world_mut().query::<&Client>();
    let count_of_clients_client = client_query_client.iter(client_app.world()).len();

    assert_eq!(
        count_of_clients_client, 0,
        "After triggering a disconnect on the client, the client entity must not exist anymore on the client"
    );
}

fn disconnect(mut commands: Commands, mut has_run: Local<bool>) {
    if *has_run {
        return;
    }

    commands.trigger(Disconnect);
    *has_run = true;
}

// TODO: Write test to ensure client doesnt exist anymore in connected clients
