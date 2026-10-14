//! The USP's light (Snake: "a light-equipped USP"): while he aims it, a
//! pool of light where the line of fire meets a wall or floor. Its size,
//! reach and colour are assumed (the game's light effect is not ported).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::body::Body;
use crate::player::Player;

/// How far the light reaches (mm), and the pool's size there.
const REACH: f32 = 15000.0;
const NEAR_RADIUS: f32 = 120.0;
const SPREAD: f32 = 0.05;
/// Lift off the surface (mm).
const LIFT: f32 = 6.0;
/// Muzzle height when aiming (mm above the feet), as `player.rs`.
const MUZZLE: f32 = 1350.0;

#[derive(Component)]
struct Pool;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, setup).add_systems(Update, update.after(crate::player::control));
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.95, 0.8, 0.35),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    commands.spawn((Pool, Mesh3d(meshes.add(disc())), MeshMaterial3d(material), Transform::default(), Visibility::Hidden));
}

fn update(
    game: Res<crate::Game>,
    inv: Res<crate::items::Inventory>,
    player: Query<(&Body, &Player)>,
    mut pool: Query<(&mut Transform, &mut Visibility), With<Pool>>,
) {
    let Ok((mut t, mut vis)) = pool.single_mut() else { return };
    let lit = player.single().ok().filter(|(_, p)| p.aiming && inv.usp && inv.weapon == 1 && !p.dead());
    let hit = lit.and_then(|(b, _)| {
        let f = b.forward();
        let from = b.pos + Vec3::Y * MUZZLE + f * 400.0;
        let to = from + f * REACH;
        let (frac, n, _) = game.collision.as_ref()?.ray_normal(from, to)?;
        Some((from.lerp(to, frac), n, frac * REACH))
    });
    let Some((at, n, dist)) = hit else {
        if *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
        return;
    };
    let n = n.normalize_or(Vec3::Y);
    let up = if n.y.abs() > 0.9 { Vec3::Z } else { Vec3::Y };
    *t = Transform::from_translation((at + n * LIFT) * SCALE)
        .looking_to(-n, up)
        .with_scale(Vec3::splat(1.0 + dist * SPREAD / NEAR_RADIUS));
    *vis = Visibility::Inherited;
}

/// A soft disc facing -Z: bright centre, dark rim (vertex alpha).
fn disc() -> Mesh {
    let r = NEAR_RADIUS * SCALE;
    let n = 16;
    let mut pos = vec![[0.0, 0.0, 0.0]];
    let mut col = vec![[1.0, 1.0, 1.0, 1.0]];
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        pos.push([a.cos() * r, a.sin() * r, 0.0]);
        col.push([0.0, 0.0, 0.0, 0.0]);
    }
    let mut idx = Vec::new();
    for i in 0..n as u32 {
        idx.extend([0, 1 + i, 1 + (i + 1) % n as u32]);
    }
    let count = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, -1.0]; count]);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    m.insert_indices(Indices::U32(idx));
    m
}
