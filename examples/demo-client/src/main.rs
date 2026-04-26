use bevy::{color::palettes::css::RED, prelude::*};
use bincode::{Decode, Encode};
use netvy::{AppComponentExt, BevyMultiplayerFrameworkPlugin, client::ConnectToServer};

const SERVER_PORT: u16 = 8080;

#[derive(Component, Decode, Encode)]
pub struct ExampleComponent(pub f32, pub f32);

fn main() {
    println!("Starting demo client");
    let mut app = App::new();

    app.add_plugins(DefaultPlugins);

    app.add_plugins(BevyMultiplayerFrameworkPlugin(netvy::AppType::Client));

    app.add_systems(Startup, (start_connect, spawn_camera, spawn_player));

    app.add_systems(Update, (change_registered_component, movement));

    app.register_component::<EntityPosition>();
    app.register_component::<Player>();

    app.run();
}

fn start_connect(mut commands: Commands) {
    commands.trigger(ConnectToServer {
        server_url: "127.0.0.1".into(),
        port: SERVER_PORT,
    });
}

fn change_registered_component(mut single_c: Single<&mut ExampleComponent>) {
    single_c.0 += 1.0;
}

/// A marker component for a player
#[derive(Component, Decode, Encode)]
pub struct Player;

#[derive(Component, Encode, Decode)]
pub struct EntityPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
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

fn spawn_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Player,
        Mesh3d(meshes.add(Capsule3d::default())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: RED.into(),
            ..Default::default()
        })),
        Transform::from_translation(Vec3::splat(0.0)),
    ));
}

fn movement(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut player_position: Single<&mut Transform, With<Player>>,
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
