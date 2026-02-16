use bevy::color::LinearRgba;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use std::collections::{HashMap, HashSet};

use crate::data::LegendData;

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// Marks a Bevy entity as a graph node, linking it to a Legend node id.
#[derive(Component)]
pub struct GraphNode3D {
    pub legend_id: u64,
    pub mat: Handle<StandardMaterial>,
}

// ---------------------------------------------------------------------------
// Layout resource — positions & velocities live here (persist across respawns)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct LayoutState {
    pub positions: HashMap<u64, Vec3>,
    velocities: HashMap<u64, Vec3>,
}

// ---------------------------------------------------------------------------
// Selection state
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct SelectedNode {
    pub id: Option<u64>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const REPULSION: f32 = 80.0;
const ATTRACTION: f32 = 0.008;
const DAMPING: f32 = 0.82;
const CENTER_GRAVITY: f32 = 0.006;
const MAX_SPEED: f32 = 1.5;
const SPAWN_RADIUS: f32 = 25.0;

// ---------------------------------------------------------------------------
// Sync: spawn / despawn / update entities to match LegendData
// ---------------------------------------------------------------------------

pub fn sync_graph_entities(
    mut commands: Commands,
    mut data: ResMut<LegendData>,
    existing: Query<(Entity, &GraphNode3D)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut layout: ResMut<LayoutState>,
) {
    if !data.dirty {
        return;
    }
    data.dirty = false;

    let wanted: HashSet<u64> = data.graph_nodes.iter().map(|n| n.id).collect();
    let mut alive: HashMap<u64, Entity> = HashMap::new();

    // --- despawn removed nodes ---
    for (entity, gn) in &existing {
        if wanted.contains(&gn.legend_id) {
            alive.insert(gn.legend_id, entity);
        } else {
            commands.entity(entity).despawn_recursive();
            layout.positions.remove(&gn.legend_id);
            layout.velocities.remove(&gn.legend_id);
        }
    }

    let sphere: Handle<Mesh> = meshes.add(Sphere::new(1.0));

    for node in &data.graph_nodes {
        let color = node_color(node.weight, node.salience, &node.kind);
        let size = node_size(node.weight);

        if let Some(&entity) = alive.get(&node.id) {
            // --- update existing node ---
            if let Ok((_, gn)) = existing.get(entity) {
                if let Some(mat) = materials.get_mut(&gn.mat) {
                    mat.base_color = color;
                    mat.emissive = emissive_linear(node.salience, &node.kind);
                }
            }
            let pos = layout.positions.get(&node.id).copied().unwrap_or_default();
            commands
                .entity(entity)
                .insert(Transform::from_translation(pos).with_scale(Vec3::splat(size)));
        } else {
            // --- spawn new node ---
            let nid = node.id;
            let pos = *layout
                .positions
                .entry(node.id)
                .or_insert_with(|| seeded_position(nid));
            layout.velocities.entry(node.id).or_insert(Vec3::ZERO);

            let mat_handle = materials.add(StandardMaterial {
                base_color: color,
                emissive: emissive_linear(node.salience, &node.kind),
                perceptual_roughness: 0.6,
                metallic: 0.1,
                ..default()
            });

            commands.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(mat_handle.clone()),
                Transform::from_translation(pos).with_scale(Vec3::splat(size)),
                GraphNode3D {
                    legend_id: node.id,
                    mat: mat_handle,
                },
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Force-directed layout (runs every frame on LayoutState)
// ---------------------------------------------------------------------------

pub fn force_layout(mut layout: ResMut<LayoutState>, data: Res<LegendData>, time: Res<Time>) {
    let dt = time.delta_secs().min(0.05);
    let ids: Vec<u64> = layout.positions.keys().copied().collect();
    if ids.len() < 2 {
        return;
    }

    let mut forces: HashMap<u64, Vec3> = ids.iter().map(|&id| (id, Vec3::ZERO)).collect();

    // --- repulsion (all pairs) ---
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let a = ids[i];
            let b = ids[j];
            let pa = layout.positions[&a];
            let pb = layout.positions[&b];
            let delta = pa - pb;
            let dist = delta.length().max(0.5);
            let mag = REPULSION / (dist * dist);
            let dir = delta / dist;
            *forces.get_mut(&a).unwrap() += dir * mag;
            *forces.get_mut(&b).unwrap() -= dir * mag;
        }
    }

    // --- attraction along edges ---
    for edge in &data.graph_edges {
        let (Some(&pf), Some(&pt)) = (
            layout.positions.get(&edge.from),
            layout.positions.get(&edge.to),
        ) else {
            continue;
        };
        let delta = pt - pf;
        let dist = delta.length().max(0.1);
        let mag = dist * ATTRACTION * edge.weight;
        let dir = delta / dist;
        if let Some(f) = forces.get_mut(&edge.from) {
            *f += dir * mag;
        }
        if let Some(f) = forces.get_mut(&edge.to) {
            *f -= dir * mag;
        }
    }

    // --- center gravity + integrate ---
    for &id in &ids {
        let pos = layout.positions[&id];
        let gravity = -pos * CENTER_GRAVITY;
        let total = forces[&id] + gravity;

        let vel = layout.velocities.entry(id).or_insert(Vec3::ZERO);
        *vel = (*vel + total * dt) * DAMPING;
        let speed = vel.length();
        if speed > MAX_SPEED {
            *vel = vel.normalize() * MAX_SPEED;
        }
        let displacement = *vel * dt;
        if let Some(p) = layout.positions.get_mut(&id) {
            *p += displacement;
        }
    }
}

// ---------------------------------------------------------------------------
// Copy layout positions → entity transforms every frame
// ---------------------------------------------------------------------------

pub fn apply_layout_positions(
    mut query: Query<(&GraphNode3D, &mut Transform)>,
    layout: Res<LayoutState>,
) {
    for (gn, mut xf) in &mut query {
        if let Some(&pos) = layout.positions.get(&gn.legend_id) {
            xf.translation = pos;
        }
    }
}

// ---------------------------------------------------------------------------
// Draw edges — highlight those connected to selected node
// ---------------------------------------------------------------------------

pub fn draw_edges(
    data: Res<LegendData>,
    layout: Res<LayoutState>,
    selected: Res<SelectedNode>,
    mut gizmos: Gizmos,
) {
    let sel_id = selected.id;

    for edge in &data.graph_edges {
        let (Some(&pa), Some(&pb)) = (
            layout.positions.get(&edge.from),
            layout.positions.get(&edge.to),
        ) else {
            continue;
        };

        if sel_id == Some(edge.from) || sel_id == Some(edge.to) {
            // Highlighted edge: bright cyan
            gizmos.line(pa, pb, Color::srgba(0.3, 0.9, 1.0, 0.9));
        } else {
            let alpha = (edge.weight * 0.5).clamp(0.05, 0.5);
            gizmos.line(pa, pb, Color::srgba(0.55, 0.65, 0.85, alpha));
        }
    }
}

// ---------------------------------------------------------------------------
// Gentle pulsing animation on high-salience nodes
// ---------------------------------------------------------------------------

pub fn animate_nodes(
    mut query: Query<(&GraphNode3D, &mut Transform)>,
    data: Res<LegendData>,
    time: Res<Time>,
) {
    let t = time.elapsed_secs();
    let node_map: HashMap<u64, &crate::data::DumpNode> =
        data.graph_nodes.iter().map(|n| (n.id, n)).collect();

    for (gn, mut xf) in &mut query {
        if let Some(node) = node_map.get(&gn.legend_id) {
            if node.salience > 0.5 {
                let pulse = 1.0 + (t * 3.0 + node.id as f32).sin() * 0.08 * node.salience;
                let base = node_size(node.weight);
                xf.scale = Vec3::splat(base * pulse);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Click detection — raycast from camera through mouse, find closest node
// ---------------------------------------------------------------------------

pub fn node_click_detection(
    mouse_btn: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    layout: Res<LayoutState>,
    data: Res<LegendData>,
    mut selected: ResMut<SelectedNode>,
    mut egui_ctx: bevy_egui::EguiContexts,
) {
    // Only detect on left click (just pressed)
    if !mouse_btn.just_pressed(MouseButton::Left) {
        return;
    }

    // Don't pick through egui panels
    let ctx = egui_ctx.ctx_mut();
    if ctx.is_pointer_over_area() || ctx.wants_pointer_input() {
        return;
    }

    let Ok(window) = windows.get_single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_transform)) = cameras.get_single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(cam_transform, cursor_pos) else {
        return;
    };

    let ray_origin = ray.origin;
    let ray_dir = ray.direction.as_vec3();

    // Build a map of weights for sphere radii
    let node_weights: HashMap<u64, f32> = data
        .graph_nodes
        .iter()
        .map(|n| (n.id, n.weight))
        .collect();

    let mut best_hit: Option<(u64, f32)> = None;

    for (&id, &pos) in &layout.positions {
        let weight = node_weights.get(&id).copied().unwrap_or(1.0);
        let radius = node_size(weight);
        // Ray-sphere intersection
        let oc = ray_origin - pos;
        let b = oc.dot(ray_dir);
        let c = oc.dot(oc) - radius * radius;
        let discriminant = b * b - c;
        if discriminant < 0.0 {
            continue;
        }
        let t = -b - discriminant.sqrt();
        if t < 0.0 {
            // behind camera
            continue;
        }
        if best_hit.map_or(true, |(_, best_t)| t < best_t) {
            best_hit = Some((id, t));
        }
    }

    if let Some((id, _)) = best_hit {
        // Toggle: click same node again to deselect
        if selected.id == Some(id) {
            selected.id = None;
        } else {
            selected.id = Some(id);
        }
    } else {
        // Clicked empty space — deselect
        selected.id = None;
    }
}

// ---------------------------------------------------------------------------
// Highlight selected node and its edges
// ---------------------------------------------------------------------------

pub fn highlight_selected(
    query: Query<&GraphNode3D>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    data: Res<LegendData>,
    selected: Res<SelectedNode>,
) {
    let sel_id = selected.id;

    // Build neighbor set for the selected node
    let mut neighbor_ids: HashSet<u64> = HashSet::new();
    if let Some(sid) = sel_id {
        for edge in &data.graph_edges {
            if edge.from == sid {
                neighbor_ids.insert(edge.to);
            }
            if edge.to == sid {
                neighbor_ids.insert(edge.from);
            }
        }
    }

    let node_map: HashMap<u64, &crate::data::DumpNode> =
        data.graph_nodes.iter().map(|n| (n.id, n)).collect();

    for gn in &query {
        let Some(mat) = materials.get_mut(&gn.mat) else {
            continue;
        };
        let Some(node) = node_map.get(&gn.legend_id) else {
            continue;
        };

        if sel_id == Some(gn.legend_id) {
            // Selected node: bright white glow
            mat.emissive = LinearRgba::new(4.0, 4.0, 4.0, 1.0);
            mat.base_color = Color::WHITE;
        } else if sel_id.is_some() && neighbor_ids.contains(&gn.legend_id) {
            // Neighbor node: subtle highlight
            mat.emissive = LinearRgba::new(1.5, 1.5, 2.5, 1.0);
            mat.base_color = node_color(node.weight, node.salience, &node.kind);
        } else {
            // Normal node: restore original appearance
            mat.base_color = node_color(node.weight, node.salience, &node.kind);
            mat.emissive = emissive_linear(node.salience, &node.kind);
        }
    }
}

// ---------------------------------------------------------------------------
// Visual helpers
// ---------------------------------------------------------------------------

fn node_color(weight: f32, _salience: f32, kind: &str) -> Color {
    let hue = match kind {
        "Function" | "Method" => 190.0,
        "Struct" | "Class" | "Type" => 130.0,
        "Module" | "Import" => 55.0,
        "Symbol" => 30.0,
        "Summary" => 280.0,
        "Term" => 210.0,
        _ => 210.0,
    };
    let lightness = 0.35 + (weight.min(3.0) / 3.0) * 0.35;
    let saturation = 0.6;
    Color::hsl(hue, saturation, lightness)
}

fn emissive_linear(salience: f32, kind: &str) -> LinearRgba {
    if salience < 0.2 {
        return LinearRgba::BLACK;
    }
    let s = salience * 4.0;
    match kind {
        "Summary" => LinearRgba::new(s * 0.6, s * 0.2, s * 0.8, 1.0),
        "Term" => LinearRgba::new(s * 0.2, s * 0.4, s * 0.9, 1.0),
        "Symbol" => LinearRgba::new(s * 0.9, s * 0.5, s * 0.1, 1.0),
        _ => LinearRgba::new(s * 0.4, s * 0.6, s * 0.3, 1.0),
    }
}

fn node_size(weight: f32) -> f32 {
    0.3 + weight.ln_1p() * 0.5
}

fn random_position() -> Vec3 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    Vec3::new(
        rng.gen_range(-SPAWN_RADIUS..SPAWN_RADIUS),
        rng.gen_range(-SPAWN_RADIUS * 0.5..SPAWN_RADIUS * 0.5),
        rng.gen_range(-SPAWN_RADIUS..SPAWN_RADIUS),
    )
}

/// Deterministic starting position derived from node ID.
/// Ensures the same node always starts in the same spot across launches,
/// so the force-directed layout converges to a similar shape.
fn seeded_position(id: u64) -> Vec3 {
    // Use FNV-style hashing to spread nodes evenly
    let h1 = id.wrapping_mul(0x517cc1b727220a95);
    let h2 = id.wrapping_mul(0x6c62272e07bb0142);
    let h3 = id.wrapping_mul(0x9e3779b97f4a7c15);
    let x = ((h1 >> 32) as f32 / u32::MAX as f32) * 2.0 - 1.0;
    let y = ((h2 >> 32) as f32 / u32::MAX as f32) * 2.0 - 1.0;
    let z = ((h3 >> 32) as f32 / u32::MAX as f32) * 2.0 - 1.0;
    Vec3::new(
        x * SPAWN_RADIUS,
        y * SPAWN_RADIUS * 0.5,
        z * SPAWN_RADIUS,
    )
}
