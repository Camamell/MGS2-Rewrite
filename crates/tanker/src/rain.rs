//! Rain on the open decks (`chara #0016cb "rain" -n n -c c -b boxes`,
//! `rain.c` in the resident images: `cmd_0016cb`, init `FUN_00310760`,
//! act `FUN_0030fbf8`).
//!
//! The game keeps `-n` groups of 2 x 64 streaks in a 6 m cube around the
//! camera (texture 0xe5a6bc). The `-b` boxes (min xyz, max xyz) are where it
//! does not rain (under the superstructure, inside the hull, judging by
//! their placement in w00a). Here the streaks are thin camera-facing quads
//! falling and wrapping inside that cube; `-c` sets their opacity (of
//! 255). The exact fall speed and slant are not taken from the game.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::gcl::Spawn;

pub const RAIN_ACTOR: u32 = 0x0016CB;
/// Half the cube's side (mm).
const HALF: f32 = 3000.0;
const FALL: f32 = 9000.0;
const LENGTH: f32 = 350.0;
const WIDTH: f32 = 4.0;

#[derive(Clone, Debug)]
pub struct RainSpec {
    pub drops: usize,
    pub opacity: f32,
    pub boxes: Vec<(Vec3, Vec3)>,
}

pub fn from_spawn(s: &Spawn) -> Option<RainSpec> {
    let n = s.opt_int(b'n', 0).unwrap_or(16).clamp(1, 256) as usize;
    let c = s.opt_int(b'c', 0).unwrap_or(90).clamp(0, 255) as f32 / 255.0;
    let b: Vec<f32> = s.opt(b'b').unwrap_or(&[]).iter().filter_map(|v| v.int()).map(|v| v as f32).collect();
    let boxes = b
        .chunks_exact(6)
        .map(|v| {
            let (a, z) = (Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5]));
            (a.min(z), a.max(z))
        })
        .collect();
    Some(RainSpec { drops: n * 2 * 64, opacity: c, boxes })
}

#[derive(Component)]
pub struct RainField {
    drops: Vec<Vec3>,
    seed: u32,
}

fn rand(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(0x5D58_8B65).wrapping_add(1);
    *seed as f32 / u32::MAX as f32
}

/// Spawns the rain for a stage that has it (once per stage).
pub fn setup(
    mut commands: Commands,
    game: Res<crate::Game>,
    fields: Query<Entity, With<RainField>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut started: Local<String>,
) {
    if !game.is_changed() {
        return;
    }
    for e in &fields {
        commands.entity(e).try_despawn();
    }
    let Some(spec) = &game.rain else { return };
    // rain.c starts the rain loop (effect 0x1d4) with the first field.
    if *started != game.data.name {
        *started = game.data.name.clone();
        crate::sfx::play(crate::sfx::RAIN, None);
    }
    let mut seed = 0x1234_5678;
    let drops = (0..spec.drops)
        .map(|_| Vec3::new(rand(&mut seed) * 2.0 - 1.0, rand(&mut seed) * 2.0 - 1.0, rand(&mut seed) * 2.0 - 1.0) * HALF)
        .collect();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    let n = spec.drops * 4;
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 0.0, 1.0]; n]);
    let idx: Vec<u32> = (0..spec.drops as u32).flat_map(|q| [q * 4, q * 4 + 1, q * 4 + 2, q * 4 + 2, q * 4 + 1, q * 4 + 3]).collect();
    mesh.insert_indices(Indices::U32(idx));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.8, 0.85, 0.9, spec.opacity),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        RainField { drops, seed },
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::default(),
        Visibility::Hidden,
        bevy::camera::visibility::NoFrustumCulling,
    ));
}

/// Moves the streaks and rebuilds them facing the camera.
pub fn fall(
    time: Res<Time>,
    game: Res<crate::Game>,
    cam: Query<&GlobalTransform, With<crate::player::FollowCam>>,
    mut fields: Query<(&mut RainField, &Mesh3d, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let (Ok(cam), Some(spec)) = (cam.single(), &game.rain) else { return };
    let eye = cam.translation() / SCALE;
    let inside = !spec.boxes.iter().any(|(a, b)| eye.cmpge(*a).all() && eye.cmple(*b).all());
    let dt = time.delta_secs().min(0.05);
    let right = cam.right().as_vec3();
    for (mut f, mesh3d, mut vis) in &mut fields {
        let want = if inside { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if !inside {
            continue;
        }
        let f = f.as_mut();
        let mut pos = Vec::with_capacity(f.drops.len() * 4);
        for d in f.drops.iter_mut() {
            d.y -= FALL * dt;
            d.x += FALL * 0.08 * dt;
            // Keep each drop inside the cube around the eye.
            for k in 0..3 {
                if d[k] < -HALF {
                    d[k] += 2.0 * HALF;
                } else if d[k] > HALF {
                    d[k] -= 2.0 * HALF;
                }
            }
            let top = eye + *d;
            let bottom = top + Vec3::new(LENGTH * 0.08, -LENGTH, 0.0);
            let w = right * WIDTH;
            for p in [top - w, top + w, bottom - w, bottom + w] {
                pos.push((p * SCALE).to_array());
            }
        }
        let _ = rand(&mut f.seed);
        if let Some(mut m) = meshes.get_mut(&mesh3d.0) {
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        }
    }
}
