//! Bullet holes where rounds strike walls and floors. The stages create the
//! game's scar actor (`chara #64afef <name> -u 32`, `wall_scar.c`), whose
//! `-u` is taken here as how many marks stay at once; the mark itself (a
//! small dark disc) is assumed, not the game's texture.

use std::sync::Mutex;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::LevelEntity;

/// Marks kept at once.
const MAX_SCARS: usize = 32;
/// Mark radius (mm) and lift off the surface against z-fighting.
const RADIUS: f32 = 45.0;
const LIFT: f32 = 4.0;

static QUEUE: Mutex<Vec<(Vec3, Vec3)>> = Mutex::new(Vec::new());

/// A round struck a surface at `at` (mm) with the surface normal `normal`.
pub fn add(at: Vec3, normal: Vec3) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push((at, normal));
    }
}

#[derive(Component)]
struct Scar;

#[derive(Resource, Default)]
struct Assets3(Option<(Handle<Mesh>, Handle<StandardMaterial>)>, Vec<Entity>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Assets3>().add_systems(Update, spawn);
}

fn spawn(
    mut commands: Commands,
    mut assets: ResMut<Assets3>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    alive: Query<(), With<Scar>>,
) {
    let new: Vec<(Vec3, Vec3)> = QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    // Marks of an earlier stage are gone with its level entities.
    assets.1.retain(|e| alive.contains(*e));
    if new.is_empty() {
        return;
    }
    let (mesh, mat) = assets
        .0
        .get_or_insert_with(|| {
            (
                meshes.add(disc()),
                materials.add(StandardMaterial {
                    base_color: Color::srgba(0.05, 0.05, 0.05, 0.85),
                    alpha_mode: AlphaMode::Blend,
                    unlit: true,
                    ..default()
                }),
            )
        })
        .clone();
    for (at, n) in new {
        let n = n.normalize_or(Vec3::Y);
        let place = Transform::from_translation((at + n * LIFT) * SCALE).looking_to(-n, if n.y.abs() > 0.9 { Vec3::Z } else { Vec3::Y });
        let e = commands.spawn((Scar, LevelEntity, Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), place)).id();
        assets.1.push(e);
        if assets.1.len() > MAX_SCARS {
            let old = assets.1.remove(0);
            commands.entity(old).try_despawn();
        }
    }
}

/// A small disc facing -Z (the mark's front).
fn disc() -> Mesh {
    let r = RADIUS * SCALE;
    let n = 10;
    let mut pos = vec![[0.0, 0.0, 0.0]];
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        pos.push([a.cos() * r, a.sin() * r, 0.0]);
    }
    let mut idx = Vec::new();
    for i in 0..n as u32 {
        idx.extend([0, 1 + i, 1 + (i + 1) % n as u32]);
    }
    let count = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, -1.0]; count]);
    m.insert_indices(Indices::U32(idx));
    m
}
