use std::{env, process::exit};

use bevy::{
    color::palettes::css::{RED, WHITE},
    prelude::*,
};
use netvy::{client::ConnectToServer, prelude::*};
use serde::{Deserialize, Serialize};

use crate::protocol::DemoMessage;

const SERVER_PORT: u16 = 8080;

pub struct DemoClientPlugin;

impl Plugin for DemoClientPlugin {
    fn build(&self, app: &mut App) {
        println!("Starting demo client");

        let args: Vec<String> = env::args().collect();

        if args.len() <= 1 {
            println!("Please provide a client id as first argument");
            exit(1);
        }

        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("demo-client {}", args[1]),
                ..default()
            }),
            ..default()
        }));

        app.add_plugins(NetvyPlugin(netvy::AppType::Client));

        app.add_systems(
            Startup,
            (start_connect, spawn_camera, spawn_player, spawn_map),
        );

        app.add_systems(
            Update,
            (
                movement,
                spawn_visual_for_new_player,
                read_demo_message,
                log_connection,
            ),
        );
    }
}

fn start_connect(mut commands: Commands) {
    let client_entity = commands
        .spawn((
            Client,
            TargetAddress {
                address: "0.0.0.0".to_string(),
                port: SERVER_PORT,
            },
        ))
        .id();

    commands.trigger(ConnectToServer { client_entity });
}

/// A marker component for a player
#[derive(Component, Serialize, Deserialize, Debug)]
pub struct Player;

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

/// Marker component only existing on local client to identify
#[derive(Component)]
pub struct OurEntity;

fn spawn_map(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Plane3d {
            normal: Dir3::Y,
            half_size: vec2(10.0, 10.0),
        })),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: WHITE.into(),
            ..Default::default()
        })),
        Name::new("Ground"),
    ));

    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(0.0, 10.0, 0.0),
        Name::new("Light"),
    ));
}

fn spawn_player(mut commands: Commands) {
    commands.spawn((
        Player,
        Transform::from_translation(vec3(0.0, 1.0, 0.0)),
        ReplicateEntity,
        SyncPosition::default(),
        OurEntity,
        Name::new("Our Player"),
    ));
}

fn movement(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut player_position: Single<&mut Transform, (With<Player>, With<OurEntity>)>,
    time: Res<Time>,
) {
    if keyboard_input.pressed(KeyCode::KeyW) {
        player_position.translation.z -= 1.0 * time.delta_secs();
    }
    if keyboard_input.pressed(KeyCode::KeyA) {
        player_position.translation.x -= 1.0 * time.delta_secs();
    }
    if keyboard_input.pressed(KeyCode::KeyS) {
        player_position.translation.z += 1.0 * time.delta_secs();
    }
    if keyboard_input.pressed(KeyCode::KeyD) {
        player_position.translation.x += 1.0 * time.delta_secs();
    }
}

fn spawn_visual_for_new_player(
    mut commands: Commands,
    added_players: Query<Entity, Added<Player>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for added_player in added_players {
        commands.entity(added_player).insert((
            Mesh3d(meshes.add(Capsule3d::default())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: RED.into(),
                ..Default::default()
            })),
        ));
    }
}

fn read_demo_message(mut message_reader: MessageReader<DemoMessage>) {
    for message in message_reader.read() {
        info!("Received message from server: {:?}", message);
    }
}

fn _send_demo_message(mut message_writer: MessageWriter<DemoMessage>) {
    message_writer.write(DemoMessage("Hello from client!".to_string()));
}

fn log_connection(query: Query<&ConnectionState, Changed<ConnectionState>>) {
    for connection_state in query {
        info!("ConnectionState changed! -> {connection_state:?}");
    }
}
