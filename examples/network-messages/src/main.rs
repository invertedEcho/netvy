use std::net::{Ipv4Addr, SocketAddr};

use bevy::{log::LogPlugin, prelude::*};
use netvy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Message, Serialize, Deserialize)]
struct DemoMessage(pub String);

fn main() {
    let Some(run_mode) = std::env::args().nth(1) else {
        eprintln!(
            "Please specify a run mode as first argument. Possible values: 'server' 'client'"
        );
        return;
    };

    let mut app = App::new();

    app.add_plugins(MinimalPlugins)
        .add_plugins(LogPlugin::default());

    if run_mode == "client" {
        app.add_plugins(NetvyPlugin(AppType::Client));

        app.add_systems(Startup, setup_client);
        app.add_systems(Update, write_demo_message);
    } else if run_mode == "server" {
        app.add_plugins(NetvyPlugin(AppType::Server));

        app.add_systems(Startup, setup_server);
        app.add_systems(Update, read_demo_message);
    } else {
        eprintln!("Invalid run mode: {run_mode}. Possible values: 'server' 'client'");
        return;
    }

    app.add_message::<DemoMessage>();

    // NOTE: Don't forget to register your message!
    app.register_net_message::<DemoMessage>(MessageDirection::ClientToServer);

    app.run();
}

fn setup_client(mut commands: Commands) {
    let target_address = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 1234);

    let client_entity = commands.spawn((Client, TargetAddress(target_address))).id();

    commands.trigger(ConnectToServer { client_entity })
}

fn read_demo_message(net_message_readers: Query<(&mut NetMessageReader<DemoMessage>, &PeerId)>) {
    for (mut net_message_reader, peer_id) in net_message_readers {
        for message in net_message_reader.read() {
            info!(
                "Read a demo message from {peer_id:?}, content: {}",
                message.0
            );
        }
    }
}

fn setup_server(mut commands: Commands) {
    let target_address = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 1234);

    let server_entity = commands.spawn((Server, TargetAddress(target_address))).id();

    commands.trigger(StartServer { server_entity });
}

fn write_demo_message(
    mut net_message_writer: Query<&mut NetMessageWriter<DemoMessage>, With<Client>>,
) {
    match net_message_writer.single_mut() {
        Ok(mut net_message_writer) => {
            net_message_writer.write(DemoMessage("hello from server!".to_string()));
        }
        Err(error) => {
            error!("error: {error:?}");
        }
    }
}
