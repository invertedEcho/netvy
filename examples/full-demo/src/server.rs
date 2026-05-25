use bevy::{log::LogPlugin, prelude::*};
use bevy_inspector_egui::{bevy_egui::EguiPlugin, quick::WorldInspectorPlugin};
use netvy::{client::ClientConnectionState, prelude::*, server::StartServer};

use crate::protocol::DemoMessage;

pub struct DemoServerPlugin;

impl Plugin for DemoServerPlugin {
    fn build(&self, app: &mut App) {
        let headful = if let Some(res) = std::env::args().nth(1) {
            res == "headful"
        } else {
            false
        };

        println!("Starting demo server");
        if headful {
            app.add_plugins(DefaultPlugins);
            app.add_plugins(EguiPlugin::default())
                .add_plugins(WorldInspectorPlugin::new());
        } else {
            app.add_plugins(MinimalPlugins)
                .add_plugins(LogPlugin::default());
        }

        app.add_plugins(NetvyPlugin(netvy::AppType::Server));

        app.add_systems(
            Startup,
            (start_server, spawn_camera, spawn_connection_state_text),
        );

        app.add_systems(
            Update,
            (
                update_connection_state_text.run_if(state_changed::<ClientConnectionState>),
                send_demo_message,
            ),
        );
    }
}

fn start_server(mut commands: Commands) {
    commands.trigger(StartServer { port: 8080 })
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform {
            translation: vec3(5.0, 5.0, 5.0),
            ..default()
        }
        .looking_at(Vec3::splat(0.0), Vec3::Y),
    ));
}

#[derive(Component)]
struct ClientConnectionStateText;

fn spawn_connection_state_text(mut commands: Commands) {
    commands.spawn((
        Text::new("Client Connection State:"),
        TextFont {
            font_size: 32.0,
            ..default()
        },
    ));
    commands.spawn((ClientConnectionStateText, Text::new("")));
}

fn update_connection_state_text(
    mut connection_state_text: Single<&mut Text, With<ClientConnectionStateText>>,
    client_connection_state: Res<State<ClientConnectionState>>,
) {
    ***connection_state_text = format!("{:?}", client_connection_state.get());
}

fn send_demo_message(mut message_writer: MessageWriter<DemoMessage>) {
    message_writer.write(DemoMessage("Hello from server!".to_string()));
}
