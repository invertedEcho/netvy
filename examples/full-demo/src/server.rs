use bevy::{log::LogPlugin, prelude::*};
use bevy_inspector_egui::{bevy_egui::EguiPlugin, quick::WorldInspectorPlugin};
use netvy::{prelude::*, server::StartServer};

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

        app.add_systems(Startup, (start_server, spawn_camera));

        app.add_systems(Update, (send_demo_message,));
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

fn _read_demo_message(mut message_reader: MessageReader<DemoMessage>) {
    for message in message_reader.read() {
        info!("Received message: {:?}", message);
    }
}

fn send_demo_message(mut message_writer: MessageWriter<DemoMessage>) {
    message_writer.write(DemoMessage("Hello from server!".to_string()));
}
