use bevy::{color::palettes::css::RED, prelude::*};
use bincode::{Decode, Encode};
use netvy::{AppComponentExt, NetvyPlugin, SyncEntity, SyncPosition, client::ConnectToServer};

const SERVER_PORT: u16 = 8080;

fn main() {
    println!("Starting demo client");
    let mut app = App::new();

    app.add_plugins(DefaultPlugins);

    app.add_plugins(NetvyPlugin(netvy::AppType::Client));

    app.add_systems(Startup, (start_connect, spawn_camera, spawn_player));

    app.add_systems(Update, (movement, spawn_visual_for_new_player));

    app.register_component::<Player>();

    app.run();
}

fn start_connect(mut commands: Commands) {
    commands.trigger(ConnectToServer {
        server_url: "127.0.0.1".into(),
        port: SERVER_PORT,
    });
}
/// A marker component for a player
#[derive(Component, Decode, Encode)]
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

#[derive(Component)]
pub struct OurEntity;

fn spawn_player(mut commands: Commands) {
    commands.spawn((
        Player,
        Transform::from_translation(Vec3::splat(0.0)),
        SyncEntity,
        // Insert this component to sync the position (transform.translation) of this entity to all
        // connected clients
        SyncPosition,
        OurEntity,
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
