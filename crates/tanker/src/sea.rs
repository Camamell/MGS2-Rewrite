//! The sea around the Tanker (`wave2.c`, `chara #4a777d`).
//!
//! `-t set tex1 tex2`: a texture set and two of its textures (the water and
//! a second layer drawn over it, alpha 0x44); `-p x y z` the surface (y is
//! its height), `-c r g b` its colour (128 = 1.0), `-d` the light direction,
//! `-a` ambient. The game builds two 16 x 16 vertex grids whose vertices bob
//! with random phases (`FUN_003a0198`) and scrolls the second layer
//! (`FUN_0039f998`: phase += speed / 100 a frame); the VU0 code that shapes
//! them is not ported, so this is a camera-following grid with the same
//! textures, height and colour and a gentle swell.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use mgs2_bevy::scene::{MaterialCache, SCALE};
use mgs2_formats::gcx::{Gcx, Stmt, Value};
use mgs2_formats::str_code;

/// `wave2.c`'s actor name.
const SEA: u32 = 0x4A777D;
/// Grid cells per side and cell size (mm); one texture repeat per cell.
const CELLS: usize = 48;
const CELL: f32 = 8000.0;
/// Swell height (mm) and speed (radians a second).
const SWELL: f32 = 120.0;
const SPEED: f32 = 0.9;

#[derive(Clone, Debug)]
pub struct SeaSpec {
    textures: [u32; 2],
    height: f32,
    color: Vec3,
    light: Vec3,
    ambient: Vec3,
}

/// The stage script's sea, if any.
pub fn from_script(script: &Gcx) -> Option<SeaSpec> {
    fn walk(stmts: &[Stmt]) -> Option<SeaSpec> {
        for s in stmts {
            let Stmt::Command { name, args, options } = s else { continue };
            if *name == str_code(b"chara") && matches!(args.first(), Some(Value::Name(h)) if *h == SEA) {
                let opt = |l: u8| options.iter().find(|(o, _)| *o == l).map(|(_, v)| v.as_slice()).unwrap_or(&[]);
                let name = |v: &Value| match v {
                    Value::Str(s) => Some(str_code(s.as_bytes())),
                    Value::Name(h) => Some(*h & 0x00FF_FFFF),
                    _ => None,
                };
                let nums = |l: u8| -> Vec<f32> { opt(l).iter().filter_map(|v| if let Value::Int(_, i) = v { Some(*i as f32) } else { None }).collect() };
                let (p, c, t, d, a) = (nums(b'p'), nums(b'c'), opt(b't'), nums(b'd'), nums(b'a'));
                let v3 = |v: &[f32], or: Vec3| if v.len() == 3 { Vec3::new(v[0], v[1], v[2]) } else { or };
                return Some(SeaSpec {
                    textures: [t.get(1).and_then(name).unwrap_or(0), t.get(2).and_then(name).unwrap_or(0)],
                    height: p.get(1).copied().unwrap_or(0.0),
                    color: v3(&c, Vec3::splat(128.0)) / 128.0,
                    light: v3(&d, Vec3::NEG_Y).normalize_or(Vec3::NEG_Y),
                    ambient: v3(&a, Vec3::ZERO) / 128.0,
                });
            }
            for v in args.iter().chain(options.iter().flat_map(|(_, v)| v)) {
                if let Value::Block(b) = v
                    && let Some(s) = walk(b)
                {
                    return Some(s);
                }
            }
        }
        None
    }
    walk(&script.main).or_else(|| script.procs.iter().find_map(|p| walk(&p.body)))
}

/// A sea layer: 0 the water, 1 the layer over it.
#[derive(Component)]
pub struct SeaLayer(usize);

fn grid() -> Mesh {
    let n = CELLS + 1;
    let half = CELLS as f32 * CELL * 0.5;
    let mut pos = Vec::with_capacity(n * n);
    let mut uv = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            pos.push([(i as f32 * CELL - half) * SCALE, 0.0, (j as f32 * CELL - half) * SCALE]);
            uv.push([i as f32, j as f32]);
        }
    }
    let mut idx = Vec::with_capacity(CELLS * CELLS * 6);
    for j in 0..CELLS as u32 {
        for i in 0..CELLS as u32 {
            let a = j * n as u32 + i;
            idx.extend([a, a + n as u32, a + 1, a + 1, a + n as u32, a + n as u32 + 1]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n * n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

/// Spawns the stage's sea when a stage loads.
pub fn setup(
    mut commands: Commands,
    game: Res<crate::Game>,
    layers: Query<Entity, With<SeaLayer>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut stage: Local<String>,
) {
    if *stage == game.data.name {
        return;
    }
    *stage = game.data.name.clone();
    for e in &layers {
        commands.entity(e).try_despawn();
    }
    let Some(spec) = game.data.script.as_ref().and_then(from_script) else { return };
    let mut cache = MaterialCache::default();
    for (k, tex) in spec.textures.iter().enumerate() {
        if *tex == 0 || !game.data.textures.contains_key(tex) {
            continue;
        }
        let base = cache.get(*tex, &game.data, &mut images, &mut materials, true);
        let Some(mut mat) = materials.get(&base).cloned() else { continue };
        // Lit as the game lights it: ambient plus colour x the light on a
        // level surface.
        let c = spec.ambient + spec.color * (-spec.light.y).max(0.0);
        mat.base_color = if k == 0 { Color::srgb(c.x, c.y, c.z) } else { Color::srgba(c.x, c.y, c.z, 0x44 as f32 / 128.0) };
        mat.alpha_mode = if k == 0 { AlphaMode::Opaque } else { AlphaMode::Blend };
        mat.cull_mode = None;
        mat.double_sided = true;
        commands.spawn((
            // Not a LevelEntity: deck cutscenes share the stage's coordinates and
            // keep the sea in view.
            SeaLayer(k),
            Mesh3d(meshes.add(grid())),
            MeshMaterial3d(materials.add(mat)),
            Transform::from_xyz(0.0, (spec.height + k as f32 * 20.0) * SCALE, 0.0),
            bevy::camera::visibility::NoFrustumCulling,
        ));
    }
    info!("sea: height {} mm, textures {:06x?}", spec.height, spec.textures);
}

/// Keeps the grid under the camera (snapped to whole cells so the texture
/// stays put), bobs its vertices and scrolls the upper layer.
pub fn animate(
    time: Res<Time>,
    cams: Query<&GlobalTransform, With<crate::player::FollowCam>>,
    mut layers: Query<(&SeaLayer, &Mesh3d, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(cam) = cams.single() else { return };
    let t = time.elapsed_secs();
    let cell = CELL * SCALE;
    let snap = |v: f32| (v / cell).round() * cell;
    let (cx, cz) = (snap(cam.translation().x), snap(cam.translation().z));
    for (layer, mesh3d, mut tf) in &mut layers {
        tf.translation.x = cx;
        tf.translation.z = cz;
        let Some(mut mesh) = meshes.get_mut(&mesh3d.0) else { continue };
        let n = CELLS + 1;
        let (gi, gj) = ((cx / cell).round() as i32, (cz / cell).round() as i32);
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
            for (k, p) in pos.iter_mut().enumerate() {
                // Per-vertex phase from its world cell, so waves stay put
                // as the grid moves.
                let (i, j) = ((k % n) as i32 + gi, (k / n) as i32 + gj);
                let h = ((i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663)) & 0xFFFF) as f32 / 65535.0;
                p[1] = (h * std::f32::consts::TAU + t * SPEED * (0.7 + h * 0.6)).sin() * SWELL * SCALE;
            }
        }
        if layer.0 == 1
            && let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
        {
            let shift = (t * 0.02).fract();
            for (k, q) in uv.iter_mut().enumerate() {
                q[0] = (k % n) as f32 + shift;
                q[1] = (k / n) as f32 + shift * 0.5;
            }
        }
    }
}
