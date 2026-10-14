//! Wet footprints (`foot_print.c`). The game classifies the named areas
//! Snake stands in (`FUN_0032ced0`: `0x839ecc` 0, `0x3b4657` 1, `0x97cb18`
//! 2); stepping out of a type 0 or 2 area leaves him wet for 3000 frames
//! (the actor's `+0x58`, from `FUN_0028e5a0`), type 1 dries him at once, and
//! inside a type 0 area he leaves none (`FUN_0032cf48`). While wet, each
//! step stamps a print. The areas are trap boxes of the stage's `.hzx`
//! (w01a, w01b and w01e have them). The prints' look (a dark wet sole that
//! fades with the wetness) and how many stay are assumed. Soldiers follow
//! them (`guard.rs`, stimulus 0x10000).

use std::sync::Mutex;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::LevelEntity;

/// Frames of wetness after leaving a wet area.
const WET_FRAMES: f32 = 3000.0;
/// Prints kept at once (assumed).
const MAX_PRINTS: usize = 64;
/// A print's half length and half width (mm) and its lift off the floor.
const HALF_LENGTH: f32 = 130.0;
const HALF_WIDTH: f32 = 50.0;
const LIFT: f32 = 3.0;

/// Area classes by name (`FUN_0032ced0`).
const AREAS: [(u32, u8); 3] = [(0x839ECC, 0), (0x3B4657, 1), (0x97CB18, 2)];

/// Snake's steps queued by the motion sounds: (foot position mm, facing).
static STEPS: Mutex<Vec<(Vec3, f32)>> = Mutex::new(Vec::new());

/// Snake stepped at `at` (mm) facing `yaw`.
pub fn step(at: Vec3, yaw: f32) {
    if let Ok(mut q) = STEPS.lock() {
        q.push((at, yaw));
    }
}

/// The prints on the ground, oldest first: (place mm, already followed).
/// A soldier who sees one goes to it and marks it and every older one
/// (`FUN_00240858`).
pub static TRAIL: Mutex<Vec<(Vec3, bool)>> = Mutex::new(Vec::new());

#[derive(Resource, Default)]
struct Wet {
    /// Frames of wetness left.
    frames: f32,
    /// The area class Snake was in last frame.
    class: Option<u8>,
    prints: Vec<Entity>,
    assets: Option<(Handle<Mesh>, Vec<Handle<StandardMaterial>>)>,
}

#[derive(Component)]
struct Print;

pub fn plugin(app: &mut App) {
    app.init_resource::<Wet>().add_systems(Update, update);
}

fn update(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<crate::Game>,
    player: Query<&crate::body::Body, With<crate::player::Player>>,
    alive: Query<(), With<Print>>,
    mut wet: ResMut<Wet>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let steps: Vec<(Vec3, f32)> = STEPS.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    let before = wet.prints.len();
    wet.prints.retain(|e| alive.contains(*e));
    // A new stage: its prints (and their trail) are gone.
    if wet.prints.len() < before && wet.prints.is_empty()
        && let Ok(mut t) = TRAIL.lock()
    {
        t.clear();
    }
    let Ok(body) = player.single() else { return };
    let pos = body.pos + Vec3::Y * 10.0;
    let class = game.data.collision.as_ref().and_then(|h| {
        h.traps
            .iter()
            .filter(|t| (0..3).all(|k| t.min[k] <= pos[k] && pos[k] < t.max[k]))
            .find_map(|t| AREAS.iter().find(|(n, _)| *n == t.name & 0x00FF_FFFF).map(|(_, c)| *c))
    });
    if class != wet.class {
        if class.is_none() {
            wet.frames = match wet.class {
                Some(0 | 2) => WET_FRAMES,
                _ => 0.0,
            };
        }
        wet.class = class;
    }
    if class == Some(0) {
        wet.frames = 0.0;
    }
    // MGS2_WET=1: start wet (testing).
    if std::env::var_os("MGS2_WET").is_some() && time.elapsed_secs() < 0.5 {
        wet.frames = WET_FRAMES;
    }
    wet.frames = (wet.frames - time.delta_secs() * 60.0).max(0.0);
    if wet.frames <= 0.0 || steps.is_empty() {
        return;
    }
    // Four shades from fresh to nearly dry.
    let (mesh, shades) = wet
        .assets
        .get_or_insert_with(|| {
            let shades = (0..4)
                .map(|i| {
                    materials.add(StandardMaterial {
                        base_color: Color::srgba(0.01, 0.015, 0.02, 0.7 - i as f32 * 0.15),
                        alpha_mode: AlphaMode::Blend,
                        unlit: true,
                        cull_mode: None,
                        ..default()
                    })
                })
                .collect();
            (meshes.add(sole()), shades)
        })
        .clone();
    let shade = (((1.0 - wet.frames / WET_FRAMES) * 4.0) as usize).min(3);
    for (at, yaw) in steps {
        let floor = game.collision.as_ref().and_then(|c| c.floor_below(at.x, at.z, at.y + 300.0)).unwrap_or(at.y);
        let place = Transform::from_translation(Vec3::new(at.x, floor + LIFT, at.z) * SCALE).with_rotation(Quat::from_rotation_y(yaw));
        let e = commands.spawn((Print, LevelEntity, Mesh3d(mesh.clone()), MeshMaterial3d(shades[shade].clone()), place)).id();
        wet.prints.push(e);
        if let Ok(mut t) = TRAIL.lock() {
            t.push((Vec3::new(at.x, floor, at.z), false));
            if t.len() > MAX_PRINTS {
                t.remove(0);
            }
        }
        if wet.prints.len() > MAX_PRINTS {
            let old = wet.prints.remove(0);
            commands.entity(old).try_despawn();
        }
    }
}

/// A sole-shaped oval lying flat, its length along +Z.
fn sole() -> Mesh {
    let n = 14;
    let mut pos = vec![[0.0, 0.0, 0.0]];
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        // A little wider at the toe than at the heel.
        let w = HALF_WIDTH * if a.cos() > 0.0 { 1.0 } else { 0.8 };
        pos.push([a.sin() * w * SCALE, 0.0, a.cos() * HALF_LENGTH * SCALE]);
    }
    let mut idx = Vec::new();
    for i in 0..n as u32 {
        idx.extend([0, 1 + (i + 1) % n as u32, 1 + i]);
    }
    let count = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count]);
    m.insert_indices(Indices::U32(idx));
    m
}
