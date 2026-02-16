mod data;
mod graph;
mod panels;

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use std::path::PathBuf;

fn main() {
    let project_dir = parse_project_dir();

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Legend Dashboard".into(),
                resolution: (1600., 900.).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin)
        .insert_resource(data::ProjectDir(project_dir))
        .init_resource::<data::LegendData>()
        .init_resource::<graph::LayoutState>()
        .init_resource::<graph::SelectedNode>()
        .add_systems(Startup, setup_scene)
        .add_systems(
            Update,
            (
                data::poll_legend_data,
                graph::sync_graph_entities,
                graph::force_layout,
                graph::apply_layout_positions,
                graph::draw_edges,
                graph::animate_nodes,
                graph::node_click_detection,
                graph::highlight_selected,
                panels::ui_panels,
                orbit_camera,
            ),
        )
        .run();
}

// ---------------------------------------------------------------------------
// Scene setup
// ---------------------------------------------------------------------------

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 25.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            distance: 50.0,
            pitch: 0.4,
            yaw: 0.0,
        },
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.6, 0.5, 0.0)),
    ));

    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
    });
}

// ---------------------------------------------------------------------------
// Orbit camera
// ---------------------------------------------------------------------------

#[derive(Component)]
struct OrbitCamera {
    distance: f32,
    pitch: f32,
    yaw: f32,
}

fn orbit_camera(
    mut query: Query<(&mut OrbitCamera, &mut Transform)>,
    mouse_btn: Res<ButtonInput<MouseButton>>,
    mut motion: EventReader<MouseMotion>,
    mut scroll: EventReader<MouseWheel>,
) {
    let Ok((mut orbit, mut xf)) = query.get_single_mut() else {
        return;
    };

    if mouse_btn.pressed(MouseButton::Right) {
        for ev in motion.read() {
            orbit.yaw -= ev.delta.x * 0.003;
            orbit.pitch = (orbit.pitch - ev.delta.y * 0.003).clamp(-1.4, 1.4);
        }
    } else {
        motion.clear();
    }

    for ev in scroll.read() {
        orbit.distance = (orbit.distance - ev.y * 3.0).clamp(10.0, 200.0);
    }

    let d = orbit.distance;
    let pos = Vec3::new(
        d * orbit.pitch.cos() * orbit.yaw.sin(),
        d * orbit.pitch.sin(),
        d * orbit.pitch.cos() * orbit.yaw.cos(),
    );
    xf.translation = pos;
    xf.look_at(Vec3::ZERO, Vec3::Y);
}

// ---------------------------------------------------------------------------
// CLI argument parsing
// ---------------------------------------------------------------------------

fn parse_project_dir() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--project-dir" {
            if let Some(dir) = args.get(i + 1) {
                return PathBuf::from(dir);
            }
        }
    }
    // Default: current directory
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
