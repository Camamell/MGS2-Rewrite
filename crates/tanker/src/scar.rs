//! Bullet holes where rounds strike walls and floors (`wall_scar.c`). The
//! stages create the scar actor (`chara #64afef <name> -u 32`): `-u`
//! (default 10) is the size of its ring of quads (`FUN_002d7a08`), and the
//! `#0acf09 -n <material>` hook places one there (`FUN_002d7498`): the
//! `dankon2_alp` texture on a 100 mm square (template 0x1f76d0, +-50 mm)
//! lifted 3 mm off the surface, vertex alpha 0x40. Once four slots or fewer
//! are free the oldest marks fade, 2 a frame, and go (`FUN_002d78c8`).
//! Assumed: the quad's turn about the normal (the template's matrix rows are
//! ELF data, 0x190a40, not read), white in place of the light at the point
//! (`FUN_001273b0`), and no clamp to the struck wall's extent
//! (`FUN_002d7200`). The `#41cf4e` hook's spark burst (`FUN_003057f8` ->
//! `FUN_00334be0`: 24 particles, colour (255, 128, 128), 15 frames, not
//! decoded) is a brief flash here, its size and length assumed.

use std::sync::Mutex;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::{Game, LevelEntity};

/// The ring's size (every tanker stage passes `-u 32`).
const RING: usize = 32;
/// Half the mark's side (mm) and its lift off the surface.
const HALF: f32 = 50.0;
const LIFT: f32 = 3.0;
/// Vertex alpha at placement and its fade per frame (0x80 = opaque).
const ALPHA: f32 = 64.0;
const FADE_PER_FRAME: f32 = 2.0;
/// The scar texture (`FUN_001408b8(0x3f5d20)`).
const TEXTURE: &[u8] = b"dankon2_alp";

/// How long a spark flash lasts (s).
const SPARK_SECS: f32 = 0.08;

static QUEUE: Mutex<Vec<(Vec3, Vec3, bool, bool)>> = Mutex::new(Vec::new());

/// A shot struck a surface at `at` (mm) with the surface normal `normal`,
/// throwing sparks if `sparks` and leaving a mark if `mark`.
pub fn add(at: Vec3, normal: Vec3, sparks: bool, mark: bool) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push((at, normal, sparks, mark));
    }
}

/// A mark and its vertex alpha (0..=0x80), with its own material to fade.
#[derive(Component)]
struct Scar(f32, Handle<StandardMaterial>);

/// A spark flash and the time it has left.
#[derive(Component)]
struct Spark(f32);

#[derive(Resource, Default)]
struct Assets3 {
    /// The stage the texture was made for, the quad, the texture and the flash.
    made: Option<(String, Handle<Mesh>, Option<Handle<Image>>, AlphaMode, Handle<Mesh>, Handle<StandardMaterial>)>,
    /// Live marks, oldest first.
    ring: Vec<Entity>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Assets3>().add_systems(Update, (spawn, fade_scars, fade));
}

fn fade(mut commands: Commands, time: Res<Time>, mut sparks: Query<(Entity, &mut Spark, &mut Transform)>) {
    for (e, mut s, mut t) in &mut sparks {
        s.0 -= time.delta_secs();
        if s.0 <= 0.0 {
            commands.entity(e).despawn();
        } else {
            t.scale = Vec3::splat(1.0 + 2.0 * s.0 / SPARK_SECS);
        }
    }
}

/// `FUN_002d78c8`: with `free` slots left (`free <= 4`), the `5 - free`
/// oldest marks lose 2 of alpha a frame and leave the ring below 2.
fn fade_scars(
    mut commands: Commands,
    time: Res<Time>,
    mut assets: ResMut<Assets3>,
    mut scars: Query<&mut Scar>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    assets.ring.retain(|e| scars.contains(*e));
    let free = RING.saturating_sub(assets.ring.len());
    if free > 4 {
        return;
    }
    let step = FADE_PER_FRAME * 60.0 * time.delta_secs();
    let mut gone = 0;
    for &e in assets.ring.iter().take(5 - free) {
        let Ok(mut s) = scars.get_mut(e) else { continue };
        s.0 -= step;
        if s.0 < 2.0 {
            commands.entity(e).try_despawn();
            gone += 1;
        } else if let Some(mut m) = materials.get_mut(&s.1) {
            m.base_color.set_alpha(s.0 / 128.0);
        }
    }
    assets.ring.drain(..gone);
}

fn spawn(
    mut commands: Commands,
    game: Res<Game>,
    mut assets: ResMut<Assets3>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    alive: Query<(), With<Scar>>,
) {
    let new: Vec<(Vec3, Vec3, bool, bool)> = QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    // Marks of an earlier stage are gone with its level entities.
    assets.ring.retain(|e| alive.contains(*e));
    if new.is_empty() {
        return;
    }
    if assets.made.as_ref().is_none_or(|m| m.0 != game.data.name) {
        let tex = game.data.textures.get(&mgs2_formats::cipher::str_code(TEXTURE));
        if tex.is_none() {
            eprintln!("scar: texture {} not in {}", String::from_utf8_lossy(TEXTURE), game.data.name);
        }
        let flash = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.85, 0.5),
            emissive: LinearRgba::rgb(4.0, 3.0, 1.2),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });
        assets.made = Some((
            game.data.name.clone(),
            meshes.add(quad()),
            tex.map(|t| images.add(mgs2_bevy::scene::image(t))),
            // Vertex alpha 0x40 needs blending even for a cut-out texture.
            if tex.is_some_and(|t| t.blend == mgs2_formats::tri::Blend::Add) { AlphaMode::Add } else { AlphaMode::Blend },
            meshes.add(disc()),
            flash,
        ));
    }
    let Some((_, mesh, image, alpha_mode, disc, flash)) = assets.made.clone() else { return };
    for (at, n, sparks, mark) in new {
        let n = n.normalize_or(Vec3::Y);
        let place = Transform::from_translation((at + n * LIFT) * SCALE).looking_to(-n, if n.y.abs() > 0.9 { Vec3::Z } else { Vec3::Y });
        if sparks {
            let lift = Transform::from_translation((at + n * LIFT * 2.0) * SCALE).with_rotation(place.rotation);
            commands.spawn((Spark(SPARK_SECS), LevelEntity, Mesh3d(disc.clone()), MeshMaterial3d(flash.clone()), lift));
        }
        if !mark {
            continue;
        }
        let material = materials.add(StandardMaterial {
            base_color: if image.is_some() { Color::srgba(1.0, 1.0, 1.0, ALPHA / 128.0) } else { Color::srgba(0.05, 0.05, 0.05, ALPHA / 128.0) },
            base_color_texture: image.clone(),
            alpha_mode,
            unlit: true,
            ..default()
        });
        let e = commands.spawn((Scar(ALPHA, material.clone()), LevelEntity, Mesh3d(mesh.clone()), MeshMaterial3d(material), place)).id();
        assets.ring.push(e);
        // A full ring writes over its oldest slot.
        if assets.ring.len() > RING {
            let old = assets.ring.remove(0);
            commands.entity(old).try_despawn();
        }
    }
}

/// The mark: a square facing -Z (its front), UVs over the whole texture
/// (vertex 0 at (+50, +50) takes the rectangle's first corner).
fn quad() -> Mesh {
    let h = HALF * SCALE;
    let pos = vec![[h, h, 0.0], [-h, h, 0.0], [h, -h, 0.0], [-h, -h, 0.0]];
    let uv = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, -1.0]; 4]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_indices(Indices::U32(vec![0, 1, 2, 2, 1, 3, 0, 2, 1, 2, 3, 1]));
    m
}

/// The spark flash: a small disc facing -Z.
fn disc() -> Mesh {
    let r = 45.0 * SCALE;
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
