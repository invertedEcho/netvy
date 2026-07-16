use std::net::{Ipv4Addr, SocketAddr};

use bevy::prelude::*;
use netvy::prelude::*;

const SERVER_PORT: u16 = 5888;

fn create_test_client_app() -> App {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins);
    app.add_plugins(NetvyPlugin(NetvyMode::Client));

    app
}

fn create_test_server_app() -> App {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins);
    app.add_plugins(NetvyPlugin(NetvyMode::Server));

    app
}

#[test]
fn test_client_connect_to_server() {
    let mut server_app = create_test_server_app();

    server_app.add_systems(Startup, setup_server);

    let mut client_app = create_test_client_app();

    client_app.add_systems(Startup, spawn_client_and_connect);

    client_app.update();

    let client = client_app
        .world_mut()
        .query::<(&Client, &ConnectionState)>()
        .single(client_app.world())
        .unwrap();
    assert_eq!(
        *client.1,
        ConnectionState::Connected,
        "Client must have ConnectionState::Connected"
    );
}

fn setup_server(mut commands: Commands) {
    let socket_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), SERVER_PORT);
    commands.spawn((Server, TargetAddress(socket_addr)));
}

fn spawn_client_and_connect(mut commands: Commands) {
    let socket_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), SERVER_PORT);

    let client_entity = commands.spawn((Client, TargetAddress(socket_addr))).id();
    commands.trigger(ConnectToServer { client_entity });
}
