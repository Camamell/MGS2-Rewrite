//! Stun and chaff grenades (`grenade.c`, `blst_stn.c`, `blst_chf.c`): G
//! throws one (a stun grenade if Snake has any, else chaff).
//!
//! From the game: the fuse is 300 frames from pulling the pin
//! (`FUN_002c7450`, actor +0xBC); the stun blast (`FUN_002ff800`) calls
//! `blast.c` (`FUN_002fa760(pos, 3, 10000, 10000, .., 0xb, 6)`) with
//! effect 0x41; the chaff blast (`FUN_002ff6b8`) jams for 0x1E0 frames
//! (8 s). Assumed: the throw (7 m/s forward, 4 m/s up, Snake throws at
//! once so the whole fuse runs in flight), the stun's reach (5 m, half the
//! blast's 10000, with a clear line), how long a stunned guard stays down
//! (half a dart's sleep) and the white flash when Snake is close.
//! `MGS2_GRENADE_AT=seconds` throws a stun grenade then (testing).

use bevy::prelude::*;

use crate::body::Body;
use crate::guard::Guard;
use crate::player::Player;
use crate::{Game, SCALE};

const STUN: (i32, i32) = (2, 11);
const CHAFF: (i32, i32) = (2, 10);
const FUSE: f32 = 300.0 / 60.0;
const STUN_RANGE: f32 = 5000.0;
pub const JAM_SECONDS: f32 = 0x1E0 as f32 / 60.0;

#[derive(Component)]
struct Grenade {
    pos: Vec3,
    vel: Vec3,
    fuse: f32,
    stun: bool,
    resting: bool,
}

/// Seconds of chaff jamming left (the radar is down).
#[derive(Resource, Default)]
pub struct Jam(pub f32);

#[derive(Component)]
struct Flash(f32);

pub fn plugin(app: &mut App) {
    app.init_resource::<Jam>().add_systems(Update, (throw, fly, flash));
}

fn throw(
    mut commands: Commands,
    time: Res<Time>,
    mut tested: Local<bool>,
    keys: Res<ButtonInput<KeyCode>>,
    mut inv: ResMut<crate::items::Inventory>,
    player: Query<(&Body, &Player)>,
) {
    let Ok((b, p)) = player.single() else { return };
    // MGS2_GRENADE_AT=seconds: throw a stun grenade then (testing).
    let test = std::env::var("MGS2_GRENADE_AT").ok().and_then(|v| v.parse::<f32>().ok()).is_some_and(|t| !*tested && time.elapsed_secs() >= t);
    if test {
        *tested = true;
        *inv.counts.entry(STUN).or_default() += 1;
    }
    if !(keys.just_pressed(KeyCode::KeyG) || test) || p.dead() || p.boxed || p.hang.is_some() {
        return;
    }
    let kind = [STUN, CHAFF].into_iter().find(|k| inv.counts.get(k).is_some_and(|n| *n > 0));
    let Some(kind) = kind else { return };
    *inv.counts.entry(kind).or_default() -= 1;
    let f = b.forward();
    commands.spawn((
        crate::LevelEntity,
        Grenade { pos: b.pos + Vec3::Y * 1400.0 + f * 300.0, vel: f * 7000.0 + Vec3::Y * 4000.0, fuse: FUSE, stun: kind == STUN, resting: false },
    ));
}

#[allow(clippy::too_many_arguments)]
fn fly(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut jam: ResMut<Jam>,
    mut grenades: Query<(Entity, &mut Grenade)>,
    mut guards: Query<(&Body, &mut Guard)>,
    player: Query<&Body, With<Player>>,
    mut gizmos: Gizmos,
) {
    let dt = time.delta_secs().min(0.05);
    jam.0 = (jam.0 - dt).max(0.0);
    for (e, mut g) in &mut grenades {
        g.fuse -= dt;
        if !g.resting {
            g.vel.y -= 9800.0 * dt;
            let next = g.pos + g.vel * dt;
            match game.collision.as_ref().and_then(|c| c.ray(g.pos, next)) {
                Some(t) => {
                    g.pos = g.pos.lerp(next, t * 0.95);
                    // A floor stops it; a wall knocks it back.
                    // Bounce sound: 0x2b, the clatter of a dropped object
                    // (FUN_002b7b40).
                    crate::sfx::play(0x2B, Some(g.pos));
                    if g.vel.y < 0.0 && t > 0.0 {
                        g.vel = Vec3::new(g.vel.x * 0.3, -g.vel.y * 0.2, g.vel.z * 0.3);
                        if g.vel.length() < 800.0 {
                            g.resting = true;
                        }
                    } else {
                        g.vel = -g.vel * 0.3;
                    }
                }
                None => g.pos = next,
            }
        }
        gizmos.sphere(Isometry3d::from_translation(g.pos * SCALE), 0.05, Color::srgb(0.3, 0.4, 0.3));
        if g.fuse > 0.0 {
            continue;
        }
        commands.entity(e).try_despawn();
        if g.stun {
            crate::sfx::play(0x41, Some(g.pos));
            for (b, mut guard) in &mut guards {
                let head = b.pos + Vec3::Y * 1500.0;
                let clear = game.collision.as_ref().and_then(|c| c.ray(g.pos, head)).is_none_or(|t| t > 0.97);
                if head.distance(g.pos) < STUN_RANGE && clear {
                    if !guard.asleep() {
                        guard.knock_out();
                    }
                }
            }
            if let Ok(s) = player.single()
                && s.pos.distance(g.pos) < STUN_RANGE * 2.0
            {
                commands.spawn((
                    Flash(0.8),
                    Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), bottom: px(0), ..default() },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                    GlobalZIndex(4),
                ));
            }
        } else {
            jam.0 = JAM_SECONDS;
        }
    }
}

fn flash(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Flash, &mut BackgroundColor)>) {
    for (e, mut f, mut bg) in &mut q {
        f.0 -= time.delta_secs();
        if f.0 <= 0.0 {
            commands.entity(e).try_despawn();
        } else {
            bg.0 = Color::srgba(1.0, 1.0, 1.0, (f.0 / 0.8).min(1.0) * 0.9);
        }
    }
}
