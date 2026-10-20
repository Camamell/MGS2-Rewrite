//! Marks over the guards' heads (`control_headmark2.c`, `FUN_00374550`):
//! "!" on discovery (kind 0, which also counts toward the times spotted,
//! player data +0x158c), "?" when a noise draws one (kind 1), and the "Z"
//! of a sleeper. The game's sprites: atlases 0x2a0983 ("!") and 0x2b0983
//! ("?"), three 32 x 34 frames each, and 0xd16246 ("Z"), drawn 600 mm over
//! the head. Their tints and exact timing are not taken from the game.
//! The guard's voice line for the mark plays as it appears.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::body::Body;
use crate::guard::{Guard, State};
use crate::{Game, SCALE};

const EXCLAIM: u32 = 0x2A0983;
const QUESTION: u32 = 0x2B0983;
const SLEEP: u32 = 0xD16246;
/// Head height (mm) and the mark's lift over it.
const HEAD: f32 = 1750.0;
const LIFT: f32 = 600.0;
/// Mark height (m).
const SIZE: f32 = 0.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Exclaim,
    Question,
    Sleep,
}

/// A guard's mark: one billboard per guard, following him.
#[derive(Component)]
pub struct HeadMark {
    owner: Entity,
    kind: Option<Kind>,
    /// Seconds since the current mark appeared.
    age: f32,
}

/// Quads and materials per (kind, frame).
#[derive(Resource, Default)]
struct Sprites {
    stage: String,
    frames: Vec<(Kind, Vec<(Handle<Mesh>, Handle<StandardMaterial>)>)>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Sprites>().add_systems(Update, (attach, update));
}

fn quad(u0: f32, u1: f32, aspect: f32) -> Mesh {
    let (w, h) = (SIZE * aspect / 2.0, SIZE / 2.0);
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-w, -h, 0.0], [w, -h, 0.0], [w, h, 0.0], [-w, h, 0.0]]);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[u0, 1.0], [u1, 1.0], [u1, 0.0], [u0, 0.0]]);
    m.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    m
}

/// Gives each new guard a (hidden) mark, building the sprites for the stage.
fn attach(
    mut commands: Commands,
    game: Res<Game>,
    added: Query<Entity, Added<Guard>>,
    mut sprites: ResMut<Sprites>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if added.is_empty() {
        return;
    }
    if sprites.stage != game.data.name {
        sprites.stage = game.data.name.clone();
        sprites.frames.clear();
        for (kind, tex, frames, tint) in [
            (Kind::Exclaim, EXCLAIM, 3, Color::srgb(1.0, 0.35, 0.25)),
            (Kind::Question, QUESTION, 3, Color::srgb(1.0, 0.9, 0.3)),
            (Kind::Sleep, SLEEP, 1, Color::srgb(1.4, 1.6, 2.0)),
        ] {
            let Some(t) = game.data.textures.get(&tex) else { continue };
            let image = images.add(mgs2_bevy::scene::image(t));
            let aspect = t.width as f32 / frames as f32 / t.height as f32;
            let list = (0..frames)
                .map(|i| {
                    let mat = materials.add(StandardMaterial {
                        base_color: tint,
                        base_color_texture: Some(image.clone()),
                        unlit: true,
                        // The Z's alpha does not follow its glyph (grey on black).
                        alpha_mode: if kind == Kind::Sleep { AlphaMode::Add } else { AlphaMode::Blend },
                        cull_mode: None,
                        fog_enabled: false,
                        ..default()
                    });
                    let (u0, u1) = (i as f32 / frames as f32, (i + 1) as f32 / frames as f32);
                    (meshes.add(quad(u0, u1, aspect)), mat)
                })
                .collect();
            sprites.frames.push((kind, list));
        }
    }
    let Some((_, first)) = sprites.frames.first() else { return };
    let (mesh, mat) = first[0].clone();
    for owner in &added {
        commands.spawn((
            crate::LevelEntity,
            HeadMark { owner, kind: None, age: 0.0 },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::default(),
            Visibility::Hidden,
            bevy::camera::visibility::NoFrustumCulling,
        ));
    }
}

#[allow(clippy::type_complexity)]
fn update(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    sprites: Res<Sprites>,
    guards: Query<(&Guard, &Body)>,
    cams: Query<&Transform, (With<crate::player::FollowCam>, Without<HeadMark>)>,
    mut marks: Query<(Entity, &mut HeadMark, &mut Transform, &mut Visibility, &mut Mesh3d, &mut MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    let cam = cams.single().ok();
    for (e, mut m, mut t, mut vis, mut mesh, mut mat) in &mut marks {
        let Ok((g, b)) = guards.get(m.owner) else {
            commands.entity(e).try_despawn();
            continue;
        };
        let want = match &g.state {
            State::Discovery { .. } => Some(Kind::Exclaim),
            State::Investigate { .. } => Some(Kind::Question),
            State::Asleep { .. } => Some(Kind::Sleep),
            // A downed comrade (`FUN_00253f50(body, 0xc, 0x8000 | kind)`
            // with no line): "!" before the kick (sub-state 0x12), "?" when
            // no report is needed (0xf).
            State::Comrade { sub: 0x12, .. } => Some(Kind::Exclaim),
            State::Comrade { sub: 0xf, .. } => Some(Kind::Question),
            State::HoldupCall { sub: 0x11, .. } => Some(Kind::Exclaim),
            // A shot heard (discovery sub 3, `FUN_00237918`: the mark and
            // effect 0x53, no line; the effect plays as it starts).
            State::Call { sub: 3, .. } => Some(Kind::Exclaim),
            State::HoldupCall { sub: 10, .. } => Some(Kind::Question),
            _ => None,
        };
        let voiced = matches!(g.state, State::Discovery { .. } | State::Investigate { .. });
        if want != m.kind {
            // The voice that goes with the mark (`FUN_0021c0a0(base, guard)`:
            // base + the guard's voice, together with the mark command
            // `FUN_0023d800(guard, 0xc, 0x8000 | kind)`): the discovery
            // "!" (w00a `FUN_00237128`, discovery sub-state 2) says 0x148
            // and plays effect 0x53 at the guard; 0x140 as an
            // investigation starts (w00a
            // `FUN_0023c028`, its first frame; the 0x144 that follows the
            // look-around is played by the guard).
            let base = match want {
                Some(Kind::Exclaim) => Some(0x148),
                Some(Kind::Question) => Some(0x140),
                _ => None,
            };
            if let Some(base) = base.filter(|_| voiced) {
                crate::guard::say(base, g, b.pos);
            }
            if want == Some(Kind::Exclaim) && voiced {
                crate::sfx::play(0x53, Some(b.pos));
            }
            m.kind = want;
            m.age = 0.0;
        } else {
            m.age += dt;
        }
        // "?" stays up for a moment; "!" for the whole reaction.
        let shown = match want {
            Some(Kind::Question) => m.age < 1.5,
            Some(_) => true,
            None => false,
        };
        let new_vis = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != new_vis {
            *vis = new_vis;
        }
        let (true, Some(kind)) = (shown, want) else { continue };
        let Some((_, frames)) = sprites.frames.iter().find(|(k, _)| *k == kind) else { continue };
        // Three frames at 10 a second, then the last held.
        let i = ((m.age * 10.0) as usize).min(frames.len() - 1);
        let (fm, fmat) = &frames[i];
        if mesh.0 != *fm {
            mesh.0 = fm.clone();
        }
        if mat.0 != *fmat {
            mat.0 = fmat.clone();
        }
        let mut at = b.pos + Vec3::Y * (HEAD + LIFT);
        if kind == Kind::Sleep {
            // Lying: a Z drifts up from the head (joint 12) and fades, again
            // and again.
            let model = &game.data.models[b.model].1;
            let motion = b.motions.and_then(|i| game.data.motions[i].1.motions.get(b.motion));
            let head = mgs2_bevy::anim::pose(model, motion, b.frame, false).get(12).map_or(Vec3::ZERO, |m| Vec3::from(m.translation));
            let phase = (m.age / 1.6).fract();
            at = b.pos + Quat::from_rotation_y(b.yaw) * head + Vec3::Y * (200.0 + 700.0 * phase);
            if let Some(mut sm) = materials.get_mut(fmat) {
                let k = 1.0 - phase;
                sm.base_color = Color::srgb(1.4 * k, 1.6 * k, 2.0 * k);
            }
        }
        t.translation = at * SCALE;
        if let Some(c) = cam {
            t.rotation = c.rotation;
        }
    }
}
