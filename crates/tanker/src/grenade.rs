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
//! `MGS2_GRENADE_AT=seconds` throws a stun grenade then (testing; `12c`
//! throws chaff at 12 s).
//!
//! The soldiers' grenades (`sig_grd.c`, w03a `FUN_00290398` /
//! `FUN_002900f0` / `FUN_0028ff08`), see [`throw_frag`].

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
    app.init_resource::<Jam>().add_systems(Update, (throw, fly, flash, frags));
}

/// A soldier's grenade in flight (`sig_grd.c`). Units mm and frames.
#[derive(Component)]
struct Frag {
    pos: Vec3,
    /// mm per frame (+0x50).
    vel: Vec3,
    /// Frames since the throw (+0x1e8) and the flight's (+500).
    frames: u32,
    flight: u32,
    /// Floor contacts (+0x200): 0 in the air, 1 bounced, 2 at rest.
    contacts: u8,
    /// Frames still to run (60 per second).
    pending: f32,
}

/// Grenades thrown this frame: (hand, target, arc height).
static THROWS: std::sync::Mutex<Vec<(Vec3, Vec3, f32)>> = std::sync::Mutex::new(Vec::new());

/// Gravity on a soldier's grenade, mm per frame per frame (`FUN_0028ff08`).
const FRAG_GRAVITY: f32 = 9.0;
/// Its fuse from the throw (`FUN_0028e208` kind 3 -> `FUN_00290398(.., 1,
/// 0xb4, 0)`).
const FRAG_FUSE: u32 = 0xb4;
/// The blast (`FUN_0028fe18` type 0 -> `blast.c` `FUN_0033cef0(pos, 1, 2000,
/// 2000, 0x28, 10, 0x11, 0x31)`): a 2000 mm box, damage 0x28 halved (flags
/// without 4), no noise (flag 1), sound 0x41 (flag 8 clear).
const FRAG_REACH: f32 = 1000.0;
const FRAG_DAMAGE: i32 = 0x28 / 2;

/// A soldier throws a grenade from `hand` at `target` in an arc `height`
/// high (`FUN_002900f0`): up at sqrt(18 height) mm per frame under 9 mm per
/// frame per frame, so back at the hand's height after 2 sqrt(18 height) / 9
/// frames, when it is over the target.
pub fn throw_frag(hand: Vec3, target: Vec3, height: f32) {
    if let Ok(mut t) = THROWS.lock() {
        t.push((hand, target, height));
    }
}

/// The soldiers' grenades (`FUN_0028ff08` each frame): over the target
/// (the flight's last frame) they slow to 0.9 a frame and from then on to
/// half (+0x1ec); the first floor contact (`FUN_0028fb78`) bounces them up
/// at a tenth of their speed and halves it, the second stops them (sound
/// 0x2b each). The fuse over, the blast: Snake within the box (the
/// distance to his middle in each axis, with a clear line: assumed) loses
/// [`FRAG_DAMAGE`]. Not done: the blast's effect (bomb.c `FUN_00303750`),
/// the item's model, hits on soldiers, walls (taken: knocked back as
/// Snake's grenades are). The game's blast is at the target (+0x1b0); the
/// grenade lies there by then, so its own place is used.
fn frags(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut q: Query<(Entity, &mut Frag)>,
    mut player: Query<(&Body, &mut Player)>,
    mut gizmos: Gizmos,
    mut tested: Local<bool>,
) {
    // MGS2_FRAG_AT=seconds: a soldier's grenade thrown at Snake from 3 m
    // ahead of him then (testing).
    if !*tested
        && std::env::var("MGS2_FRAG_AT").ok().and_then(|v| v.parse::<f32>().ok()).is_some_and(|t| time.elapsed_secs() >= t)
        && let Ok((b, _)) = player.single()
    {
        *tested = true;
        throw_frag(b.pos + b.forward() * 3000.0 + Vec3::Y * 1400.0, b.pos, 1000.0);
    }
    for (hand, target, height) in THROWS.lock().map(|mut t| std::mem::take(&mut *t)).unwrap_or_default() {
        let up = (18.0 * height).sqrt();
        let flight = (2.0 * up / FRAG_GRAVITY).max(1.0);
        let vel = (target - hand) / flight + Vec3::Y * up;
        commands.spawn((crate::LevelEntity, Frag { pos: hand, vel, frames: 0, flight: flight as u32, contacts: 0, pending: 0.0 }));
    }
    let dt = time.delta_secs().min(0.05);
    for (e, mut g) in &mut q {
        g.pending += dt * 60.0;
        while g.pending >= 1.0 {
            g.pending -= 1.0;
            let over = g.frames + 1 >= g.flight;
            if g.contacts < 2 {
                let next = g.pos + g.vel;
                match game.collision.as_ref().and_then(|c| c.ray(g.pos, next)) {
                    Some(t) if g.vel.y < 0.0 => {
                        g.pos = g.pos.lerp(next, t * 0.95);
                        crate::sfx::play(0x2B, Some(g.pos));
                        if g.contacts == 0 {
                            let speed = g.vel.length();
                            g.vel = Vec3::new(g.vel.x * 0.5, speed * 0.1, g.vel.z * 0.5);
                            g.pos.y += 50.0;
                        } else {
                            g.vel = Vec3::ZERO;
                            g.pos.y += 40.0;
                        }
                        g.contacts += 1;
                    }
                    Some(t) => {
                        g.pos = g.pos.lerp(next, t * 0.95);
                        g.vel = -g.vel * 0.3;
                    }
                    None => g.pos = next,
                }
                if g.contacts < 2 {
                    g.vel.y -= FRAG_GRAVITY;
                }
            }
            if over {
                g.vel.x *= 0.9 * 0.5;
                g.vel.z *= 0.9 * 0.5;
            }
            g.frames += 1;
            if g.frames >= FRAG_FUSE {
                break;
            }
        }
        gizmos.sphere(Isometry3d::from_translation(g.pos * SCALE), 0.05, Color::srgb(0.25, 0.3, 0.2));
        if g.frames < FRAG_FUSE {
            continue;
        }
        commands.entity(e).try_despawn();
        crate::sfx::play(0x41, Some(g.pos));
        info!("a soldier's grenade goes off at {}", g.pos);
        if let Ok((b, mut p)) = player.single_mut() {
            let mid = b.pos + Vec3::Y * 900.0;
            let d = (mid - g.pos).abs();
            let clear = game.collision.as_ref().and_then(|c| c.ray(g.pos + Vec3::Y * 100.0, mid)).is_none();
            if d.max_element() <= FRAG_REACH && clear && !p.dead() {
                info!("Snake caught in the blast");
                // A blast's hit (group 0x20000) knocks him down, pushed
                // away from it by an eighth of its box's half-size a frame
                // (`FUN_003724d0` / `FUN_001082f0`, record type 4).
                let away = Vec3::new(mid.x - g.pos.x, 0.0, mid.z - g.pos.z).normalize_or_zero();
                p.knocked_down(FRAG_DAMAGE, away * FRAG_REACH / 8.0);
            }
        }
    }
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
    // MGS2_GRENADE_AT=seconds[c]: throw a stun (chaff) grenade then (testing).
    let at = std::env::var("MGS2_GRENADE_AT").unwrap_or_default();
    let chaff = at.ends_with('c');
    let test = at.trim_end_matches('c').parse::<f32>().ok().is_some_and(|t| !*tested && time.elapsed_secs() >= t);
    if test {
        *tested = true;
        if chaff {
            inv.counts.remove(&STUN);
        }
        *inv.counts.entry(if chaff { CHAFF } else { STUN }).or_default() += 1;
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
            // `blast.c` makes noise 11: soldiers go to Alert a second later.
            crate::guard::noise(11, g.pos);
            for (b, mut guard) in &mut guards {
                let head = b.pos + Vec3::Y * 1500.0;
                let clear = game.collision.as_ref().and_then(|c| c.ray(g.pos, head)).is_none_or(|t| t > 0.97);
                if head.distance(g.pos) < STUN_RANGE && clear && !guard.inactive {
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
