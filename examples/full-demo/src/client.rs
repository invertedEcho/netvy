use std::{env, process::exit};

use bevy::{
    color::palettes::css::{RED, WHITE},
    prelude::*,
};
use bevy_inspector_egui::{
    bevy_egui::{EguiContext, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext},
    egui,
    quick::WorldInspectorPlugin,
};
use netvy::{
    SyncEntity, client::ConnectToServer, component_updates::UpdateSequenceMap, prelude::*,
    sync_position::SyncPosition,
};
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

        app.add_plugins(EguiPlugin::default())
            .add_plugins(WorldInspectorPlugin::new());

        app.add_plugins(NetvyPlugin(netvy::AppType::Client));

        app.add_systems(
            Startup,
            (start_connect, spawn_camera, spawn_player, spawn_map),
        );

        app.add_systems(
            Update,
            (movement, spawn_visual_for_new_player, send_demo_message),
        );

        app.add_systems(EguiPrimaryContextPass, _update_sequence_inspector);
    }
}

fn start_connect(mut commands: Commands) {
    commands.trigger(ConnectToServer {
        server_url: "0.0.0.0".into(),
        port: SERVER_PORT,
    });
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
        SyncEntity,
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

fn _update_sequence_inspector(world: &mut World) {
    let mut ui_ctx = match world
        .query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>()
        .single_mut(world)
    {
        Ok(ctx) => ctx.clone(),
        _ => return,
    };

    egui::Window::new("UpdateSequence Inspector").show(ui_ctx.get_mut(), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let update_sequence = world.resource::<UpdateSequenceMap>();
            for (key, value) in &update_sequence.0 {
                ui.horizontal(|ui| {
                    ui.label(format!("NetEntityId {:?}", key.0));
                    ui.label(format!("ComponentTypeId {:?}", key.1));
                    ui.label(format!("sequence_number {:?}", value));
                });
                ui.separator();
            }
        })
    });
}

fn read_demo_message(mut message_reader: MessageReader<DemoMessage>) {
    for message in message_reader.read() {
        info!("Received message: {:?}", message);
    }
}

fn send_demo_message(mut message_writer: MessageWriter<DemoMessage>) {
    message_writer.write(DemoMessage("Hello from client!".to_string()));
}
