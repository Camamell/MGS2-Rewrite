//! Stage fog (`chara #a53aa7 -c r g b -n near -f far`, `d_fog_set.c`).
//!
//! The actor eases the engine's fog (`FUN_0011e368` colour,
//! `FUN_0011e380` near / far in mm) to its values; without one the engine
//! keeps black fog 1.3 km away (`FUN_0011e3d0`), i.e. none. Every Tanker
//! stage sets one (w00a: 16 32 27, -1.5 m to 24 m). Drawn as linear distance
//! fog on the camera.

use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::gcl::Spawn;

pub const FOG_ACTOR: u32 = 0xA53AA7;

#[derive(Clone, Copy, Debug)]
pub struct FogSpec {
    pub color: [u8; 3],
    pub near: f32,
    pub far: f32,
}

pub fn from_spawn(s: &Spawn) -> Option<FogSpec> {
    let c = |i| s.opt_int(b'c', i).map(|v| v.clamp(0, 255) as u8);
    Some(FogSpec {
        color: [c(0)?, c(1)?, c(2)?],
        near: s.opt_int(b'n', 0)? as f32,
        far: s.opt_int(b'f', 0)? as f32,
    })
}

/// Puts the fog on the camera: the stage's, or during a cutscene the demo's
/// own (type-6 effect 0x1000000; none if the demo sets none).
pub fn apply(
    mut commands: Commands,
    game: Res<crate::Game>,
    cutscene: Res<crate::demo::Cutscene>,
    cams: Query<Entity, With<crate::player::FollowCam>>,
    mut shown: Local<Option<Option<(u32, [u8; 3], i32, i32)>>>,
) {
    let Ok(cam) = cams.single() else { return };
    let want = if cutscene.active() { cutscene.fog() } else { game.fog };
    let key = want.map(|f| (0, f.color, f.near as i32, f.far as i32));
    if *shown == Some(key) {
        return;
    }
    *shown = Some(key);
    // MGS2_FOG_LOG=1: each fog change (checking the cutscenes' fog keys).
    if std::env::var_os("MGS2_FOG_LOG").is_some() {
        info!("fog {:?} (cutscene {:?})", want.map(|f| (f.color, f.near, f.far)), cutscene.position());
    }
    match want {
        Some(f) => {
            let [r, g, b] = f.color;
            commands.entity(cam).insert(DistanceFog {
                color: Color::srgb_u8(r, g, b),
                falloff: FogFalloff::Linear { start: f.near.max(0.0) * SCALE, end: f.far.max(f.near + 1.0) * SCALE },
                ..default()
            });
        }
        None => {
            commands.entity(cam).remove::<DistanceFog>();
        }
    }
}
/// Models drawn without fog (the skyline far beyond the fog's end).
#[derive(Component)]
pub struct NoFog;

/// Gives a NoFog model's meshes copies of their materials without fog.
pub fn unfog(
    added: Query<Entity, Added<NoFog>>,
    children: Query<&Children>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for root in &added {
        for e in std::iter::once(root).chain(children.iter_descendants(root)) {
            let Ok(mut m) = mats.get_mut(e) else { continue };
            let Some(mut mat) = materials.get(&m.0).cloned() else { continue };
            mat.fog_enabled = false;
            m.0 = materials.add(mat);
        }
    }
}