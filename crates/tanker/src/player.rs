//! Snake: movement, collision and the follow camera.

use std::f32::consts::TAU;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::Game;
use crate::body::{Body, turn_toward};

/// Motions in `snake.mar` identified by their root motion (see
/// `re/scripts/mar_root.py`): hip height and travel speed.
pub mod motion {
    pub const IDLE: usize = 0;
    pub const CROUCH_IDLE: usize = 5;
    pub const WALK: usize = 17;
    pub const RUN: usize = 18;
    pub const CROUCH_WALK: usize = 53;
    /// Lying flat, and crawling forward (620 mm per 80 frames).
    pub const PRONE_IDLE: usize = 7;
    pub const CRAWL: usize = 8;
    /// Collapsing onto his side (270 frames, hip 860 -> 160 mm).
    pub const DEATH: usize = 13;
    /// Two-handed aiming stance.
    pub const AIM: usize = 2;
    /// A jab.
    pub const PUNCH: usize = 23;
    /// Hanging by the hands, and moving along while hanging.
    pub const HANG: usize = 20;
    pub const SHIMMY: usize = 21;
    /// Back to a wall (standing / crouched): idle, sidesteps to his right
    /// (+x) and left, a knock. Identified from their poses and root travel.
    pub const WALL: usize = 55;
    pub const WALL_RIGHT: usize = 57;
    pub const WALL_LEFT: usize = 58;
    pub const WALL_KNOCK: usize = 67;
    pub const WALL_CROUCH: usize = 79;
    pub const WALL_CROUCH_RIGHT: usize = 81;
    pub const WALL_CROUCH_LEFT: usize = 82;
    pub const WALL_CROUCH_KNOCK: usize = 91;
    /// Forward roll out of a run (52 frames, 625 mm forward).
    pub const ROLL: usize = 117;
    /// Leaning out at a wall's end (the next motion holds it).
    pub const PEEK_RIGHT: usize = 61;
    pub const PEEK_LEFT: usize = 63;
}

/// Travel speeds measured from those motions' root tracks (mm/s).
const WALK_SPEED: f32 = 1250.0;
pub const RUN_SPEED: f32 = 3050.0;
const CROUCH_SPEED: f32 = 1250.0;
const CRAWL_SPEED: f32 = 465.0;
/// Hanging: ledge index, position along it (0..1) and the grip gauge
/// (1 = full).
#[derive(Clone, Copy, Debug)]
pub struct Hang {
    pub line: usize,
    pub t: f32,
    pub grip: f32,
}

/// How far from a ledge Snake can grab it, and where he hangs (mm).
const HANG_REACH: f32 = 700.0;
const HANG_OUT: f32 = 250.0;
/// Feet below the deck while hanging (hands at deck level).
const HANG_DROP: f32 = 1950.0;
const SHIMMY_SPEED: f32 = 500.0;
/// Grip gauge length at the starting grip level (an assumption).
const GRIP_SECONDS: f32 = 25.0;
/// Landing (`FUN_002898d0` picks it by the drop): under 3200 mm a soft
/// landing; up to 6400 mm a hard one (`FUN_00288d58`) that costs
/// drop / 100 - 20 life (never below 1), thuds (effect 0x47) and makes noise
/// 6; from 6400 mm the fall kills (`FUN_00288e70`, noise 10).
const HARD_LANDING: f32 = 3200.0;
const FATAL_FALL: f32 = 6400.0;

/// `--hang-at`: press Space at this time (seconds), for tests.
#[derive(Resource)]
pub struct AutoHang(pub f32);

/// The farthest a scripted walk is taken to go (mm).
const SCRIPT_WALK_MAX: f32 = 8000.0;

/// Turning while aiming (radians per second).
const AIM_TURN: f32 = 1.6;
const TURN_RATE: f32 = TAU * 1.5;
pub const RADIUS: f32 = 300.0;
pub const HEIGHT: f32 = 1700.0;
pub const STEP_UP: f32 = 450.0;
const GRAVITY: f32 = 9800.0;

#[derive(Component, Default)]
pub struct Player {
    pub fall_speed: f32,
    pub crouch: bool,
    pub prone: bool,
    /// Camera forward (xz) captured when the current input began.
    pub basis: Option<Vec2>,
    /// Current horizontal speed (mm/s); guards hear running.
    pub speed: f32,
    /// Life (script Bs16:0xf2, player+0x782) and its maximum (Bs16:0xf4).
    pub life: i32,
    pub max_life: i32,
    pub rations: u32,
    /// Seconds of hit flash left.
    pub hurt: f32,
    /// Seconds since the last damage (`FUN_0026b650` compares the frame
    /// clock with +0xfa8).
    pub since_hit: f32,
    pub very_easy: bool,
    /// Holding the M9 up.
    pub aiming: bool,
    /// Seconds left of an action (a punch) that holds Snake in place.
    pub busy: f32,
    pub hang: Option<Hang>,
    /// Height (mm) a fall started from, for landing damage.
    pub fall_from: Option<f32>,
    /// In the cardboard box.
    pub boxed: bool,
    /// Back to a wall: the direction away from it (xz).
    pub wall: Option<Vec2>,
    /// Seconds spent pushing into a wall (presses at [`WALL_PRESS`]).
    pub wall_push: f32,
    /// Dripping blood (life under a third after a hit), until life is back
    /// to a third or more.
    pub bleeding: bool,
    /// Seconds spent moving without getting anywhere (the walkable area's
    /// edge): he stands instead of running on the spot.
    pub blocked: f32,
    /// Frames left of a forward roll (motion 117).
    pub roll: f32,
}

/// Roll travel: motion 117 moves 625 mm in 52 frames.
const ROLL_SPEED: f32 = 624.6 / 52.0 * 60.0;
/// Pushing into a wall this long puts Snake's back to it (assumed).
const WALL_PRESS: f32 = 0.25;
/// Sidestep along a wall: motions 57 / 58 travel 453 mm in 48 frames.
const WALL_STEP_SPEED: f32 = 453.0 / 48.0 * 60.0;

/// Maximum life by difficulty, from `n_title`'s difficulty select
/// (Very Easy 200, Easy 150, Normal 100, Hard 75, Extreme 50).
pub fn max_life(difficulty: i16) -> i32 {
    match difficulty {
        ..=10 => 200,
        11..=20 => 150,
        21..=30 => 100,
        31..=40 => 75,
        _ => 50,
    }
}

impl Player {
    pub fn new(difficulty: i16) -> Self {
        let max = max_life(difficulty);
        Self { life: max, max_life: max, rations: 0, very_easy: difficulty <= 10, ..default() }
    }

    /// Body height for collision and bullets (mm): standing, crouching,
    /// prone (hip heights 0.8 / 0.42 / 0.14 m from the motions).
    pub fn height(&self) -> f32 {
        if self.prone {
            400.0
        } else if self.crouch {
            1000.0
        } else {
            HEIGHT
        }
    }

    pub fn dead(&self) -> bool {
        self.life <= 0
    }

    /// Damage with the ration revive of `FUN_00289320`: an equipped ration
    /// restores full life on Very Easy, half otherwise.
    pub fn damage(&mut self, amount: i32) {
        // MGS2_GOD=1: no damage (testing).
        if amount > 0 && std::env::var_os("MGS2_GOD").is_some() {
            return;
        }
        self.life -= amount;
        // A hit that leaves him under a third of his life starts him
        // bleeding (`FUN_002713e8` sets 0x1000000; `blood_dp.c`).
        if amount > 0 && self.life > 0 && self.life < self.max_life / 3 {
            self.bleeding = true;
        }
        self.hurt = 0.25;
        self.since_hit = 0.0;
        crate::sfx::play(crate::sfx::HURT, None);
        if self.life <= 0 && self.rations > 0 {
            self.rations -= 1;
            self.life = if self.very_easy { self.max_life } else { self.max_life / 2 };
        }
    }
}

#[derive(Component)]
pub struct FollowCam {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    /// Distance after pulling in for obstacles.
    pub current: f32,
    /// Use the stage script's cameras where it sets them (F3 toggles).
    pub scripted: bool,
    /// Point looked at (metres), eased while a script camera is on.
    pub look: Vec3,
    /// Follow the n-th guard instead of Snake (debugging).
    pub guard: Option<usize>,
}

/// Synthetic input for automated runs (`--auto-walk`).
#[derive(Resource)]
pub struct AutoWalk {
    pub from: f32,
    pub seconds: f32,
}

/// `--fire-at`/`--aim-for`: aim from this time for this long, then fire.
#[derive(Resource)]
pub struct AutoFire(pub f32, pub f32);

/// Muzzle height when aiming (mm above the feet, approximate).
const MUZZLE: f32 = 1350.0;

#[allow(clippy::too_many_arguments)]
pub fn control(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    auto: Option<Res<AutoWalk>>,
    auto_fire: Option<Res<AutoFire>>,
    auto_hang: Option<Res<AutoHang>>,
    mut inv: ResMut<crate::items::Inventory>,
    mut gunshot: ResMut<crate::guard::Gunshot>,
    mut game: ResMut<Game>,
    cam: Query<&Transform, With<FollowCam>>,
    stick: Option<Res<crate::pad::Stick>>,
    mut q: Query<(&mut Player, &mut Body)>,
) {
    let Ok((mut p, mut body)) = q.single_mut() else { return };
    // A soldier's rifle butt (`guard.rs` close quarters).
    let struck = crate::guard::STRIKES.swap(0, std::sync::atomic::Ordering::Relaxed);
    if struck > 0 && !p.dead() {
        p.damage(struck);
    }
    let Ok(cam_tf) = cam.single() else { return };
    let dt = time.delta_secs().min(0.05);
    p.hurt = (p.hurt - dt).max(0.0);
    if p.dead() {
        p.speed = 0.0;
        body.play_once(motion::DEATH);
        return;
    }
    // MGS2_DROP=mm: two seconds in, lift him that high to fall (testing).
    if let Some(d) = std::env::var("MGS2_DROP").ok().and_then(|v| v.parse::<f32>().ok())
        && (2.0..2.5).contains(&time.elapsed_secs())
        && p.fall_from.is_none()
    {
        body.pos.y += d;
        p.fall_from = Some(body.pos.y);
        p.fall_speed = 1.0;
    }
    // Rolling: carried forward by the motion's travel, then crouched.
    if p.roll > 0.0 {
        p.roll -= dt * 60.0;
        let f = body.forward();
        let mut next = body.pos + f * ROLL_SPEED * dt;
        if let Some(col) = &game.collision {
            next = col.push_out(next, RADIUS, 1000.0, STEP_UP);
            next = col.push_out_script(next, RADIUS, 1000.0, STEP_UP);
            if col.floor_below(next.x, next.z, next.y + STEP_UP).is_none() {
                next = body.pos;
            }
        }
        body.pos = next;
        p.speed = ROLL_SPEED;
        if p.roll <= 0.0 {
            p.crouch = true;
        }
        return;
    }
    if p.busy > 0.0 {
        p.busy -= dt;
        p.speed = 0.0;
        return;
    }

    // Hanging from a ledge under a railing.
    let hang_key = keys.just_pressed(KeyCode::Space) || auto_hang.as_ref().is_some_and(|h| h.0 <= time.elapsed_secs() && time.elapsed_secs() < h.0 + dt * 1.5);
    if let Some(mut h) = p.hang {
        let line = game.hang_lines[h.line].clone();
        p.speed = 0.0;
        h.grip -= dt / GRIP_SECONDS;
        let inward = -line.out;
        if hang_key {
            // Pull up onto the deck.
            let at = line.a.lerp(line.b, h.t);
            body.pos = Vec3::new(at.x + inward.x * 450.0, at.y, at.z + inward.y * 450.0);
            p.hang = None;
            return;
        }
        if h.grip <= 0.0 || keys.just_pressed(KeyCode::KeyZ) {
            // Let go: falls from here (see the landing check below).
            p.hang = None;
            p.fall_from = Some(body.pos.y);
            // Nothing below: over the side of the ship.
            let below = game.collision.as_ref().and_then(|c| c.floor_below(body.pos.x, body.pos.z, body.pos.y));
            if below.is_none() {
                p.life = 0;
                info!("Snake fell overboard");
            }
            return;
        }
        // Shimmy: A/D move left/right as Snake sees it (he faces the deck).
        let mut dir = 0.0;
        if keys.pressed(KeyCode::KeyA) {
            dir -= 1.0;
        }
        if keys.pressed(KeyCode::KeyD) {
            dir += 1.0;
        }
        let along = Vec2::new(line.b.x - line.a.x, line.b.z - line.a.z).normalize_or_zero();
        // Snake's right when facing `inward`.
        let right = Vec2::new(-inward.y, inward.x);
        let sign = if along.dot(right) >= 0.0 { 1.0 } else { -1.0 };
        let len = line.length().max(1.0);
        h.t = (h.t + dir * sign * SHIMMY_SPEED * dt / len).clamp(0.0, 1.0);
        let at = line.a.lerp(line.b, h.t);
        body.pos = Vec3::new(at.x + line.out.x * HANG_OUT, at.y - HANG_DROP, at.z + line.out.y * HANG_OUT);
        body.yaw = inward.x.atan2(inward.y);
        body.play(if dir != 0.0 { motion::SHIMMY } else { motion::HANG });
        p.hang = Some(h);
        return;
    }
    if hang_key && !p.crouch && game.entrance <= 0.0 {
        let near = game
            .hang_lines
            .iter()
            .enumerate()
            .map(|(i, l)| (i, l.closest(body.pos), l))
            .filter(|(_, (_, d), l)| *d < HANG_REACH && (body.pos.y - l.a.y).abs() < 150.0)
            .min_by(|a, b| a.1.1.total_cmp(&b.1.1));
        if let Some((i, (t, _), _)) = near {
            p.hang = Some(Hang { line: i, t, grip: 1.0 });
            return;
        }
    }

    // 1 / 2: the M9 or (once Snake has it) the USP.
    if keys.just_pressed(KeyCode::Digit1) {
        inv.weapon = 0;
    }
    if keys.just_pressed(KeyCode::Digit2) && inv.usp {
        inv.weapon = 1;
    }
    // Hold F to aim along the camera's view, release to fire.
    let t = time.elapsed_secs();
    let aim = (keys.pressed(KeyCode::KeyF) || auto_fire.as_ref().is_some_and(|a| (a.0..a.0 + a.1).contains(&t)))
        && game.entrance <= 0.0;
    if aim {
        p.aiming = true;
        p.speed = 0.0;
        // Aim where he faces; A/D turn (the camera may swing on its own).
        for (k, s) in [(KeyCode::KeyA, 1.0), (KeyCode::KeyD, -1.0)] {
            if keys.pressed(k) {
                body.yaw += s * AIM_TURN * dt;
            }
        }
        body.play(motion::AIM);
        return;
    }
    if std::mem::take(&mut p.aiming) {
        let usp = inv.weapon == 1 && inv.usp;
        let ammo = inv.counts.entry(if usp { crate::items::USP_AMMO } else { crate::items::M9_AMMO }).or_default();
        if *ammo > 0 {
            *ammo -= 1;
            let f = body.forward();
            let muzzle = body.pos + Vec3::Y * MUZZLE + f * 400.0;
            if usp {
                crate::bullet::fire_round(&mut commands, muzzle, f);
                crate::sfx::play(crate::sfx::USP, Some(body.pos));
                gunshot.0 = Some((body.pos, crate::guard::GUNSHOT_RANGE));
                crate::guard::noise(10, body.pos);
            } else {
                crate::bullet::fire_dart(&mut commands, muzzle, f);
                crate::sfx::play(crate::sfx::M9, Some(body.pos));
                // A suppressed shot makes noise 5 (an open one 10): the M9 is
                // made suppressed (w00b `FUN_002c6790` passes 1 to the gun
                // constructor `FUN_002c6638`, kept at +0x250; the USP's
                // `FUN_002c68b0` passes 0). The nearest soldier in earshot
                // comes to look.
                crate::guard::noise(5, body.pos);
            }
        } else {
            crate::sfx::play(crate::sfx::EMPTY, Some(body.pos));
        }
    }

    let mut input = Vec2::ZERO;
    for (k, v) in [
        (KeyCode::KeyW, Vec2::Y),
        (KeyCode::KeyS, Vec2::NEG_Y),
        (KeyCode::KeyD, Vec2::X),
        (KeyCode::KeyA, Vec2::NEG_X),
    ] {
        if keys.pressed(k) {
            input += v;
        }
    }
    // A gamepad's stick steers freely (not just the eight key directions).
    if let Some(s) = stick.as_ref().map(|s| s.0).filter(|s| *s != Vec2::ZERO) {
        input = s.normalize();
    }
    if auto.as_ref().is_some_and(|a| (a.from..a.from + a.seconds).contains(&time.elapsed_secs())) {
        input = Vec2::Y;
    }
    // C while running: a forward roll (as in the game).
    // MGS2_ROLL_AT=seconds (testing): press C then.
    let roll_test = std::env::var("MGS2_ROLL_AT").ok().and_then(|v| v.parse::<f32>().ok()).is_some_and(|t| (t..t + dt * 1.5).contains(&time.elapsed_secs()));
    if (keys.just_pressed(KeyCode::KeyC) || roll_test) && p.speed >= RUN_SPEED && !p.crouch && !p.boxed && p.wall.is_none() {
        p.roll = 52.0;
        body.play_once(motion::ROLL);
        return;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        // From prone, C gets back up to a crouch.
        p.crouch = !p.crouch || p.prone;
        p.prone = false;
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        p.prone = !p.prone;
        p.crouch = p.prone;
    }
    let walking = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    // Camera-relative direction on the ground plane.
    // Screen-relative: up on the stick is away from the camera (its view
    // direction, or its up vector when it looks straight down).
    let (f, u) = (cam_tf.forward(), cam_tf.up());
    let flat = Vec2::new(f.x, f.z);
    let fwd = if flat.length() > 0.3 { flat.normalize() } else { Vec2::new(u.x, u.z).normalize_or(Vec2::NEG_Y) };
    // Keep the direction basis while a direction is held, so a camera cut
    // does not turn Snake around mid-run.
    let fwd = if input == Vec2::ZERO {
        p.basis = None;
        fwd
    } else {
        *p.basis.get_or_insert(fwd)
    };
    let right = Vec2::new(-fwd.y, fwd.x);
    let mut dir = (fwd * input.y + right * input.x).normalize_or_zero();
    // MGS2_WALK_DIR=angle (4096 = full turn, 0 = +z): the auto-walk heads
    // that way instead of up the screen (testing).
    if auto.as_ref().is_some_and(|a| (a.from..a.from + a.seconds).contains(&time.elapsed_secs()))
        && let Some(a) = std::env::var("MGS2_WALK_DIR").ok().and_then(|v| v.parse::<f32>().ok())
    {
        let yaw = a / 4096.0 * std::f32::consts::TAU;
        dir = Vec2::new(yaw.sin(), yaw.cos());
    }
    // Walking in through a door: straight ahead, no control yet.
    let entering = game.entrance > 0.0;
    let walking = walking || entering;
    if entering {
        let f = body.forward();
        dir = Vec2::new(f.x, f.z).normalize_or_zero();
    }
    // A scripted walk: toward its point, no control; at the point, the
    // facing.
    let mut walking = walking;
    if let Some((to, run, face)) = game.script_walk {
        let d = to - Vec2::new(body.pos.x, body.pos.z);
        // (Door arrivals pass places from expressions the port does not
        // evaluate right yet; a target far off is dropped.)
        if d.length() > SCRIPT_WALK_MAX {
            warn!("scripted walk to {to} dropped ({:.0} mm away)", d.length());
            game.script_walk = None;
        } else if d.length() < 120.0 || p.blocked > 0.5 {
            body.yaw = face;
            game.script_walk = None;
            if let Some(then) = game.script_walk_then.take() {
                game.codec_then.push(then);
            }
            dir = Vec2::ZERO;
        } else {
            dir = d.normalize();
            walking = !run;
        }
    }

    // MGS2_WALK_DIR=x,z (testing): walk that world direction meanwhile.
    // (Several segments: `x,z@until;x,z@until` seconds.)
    if let Ok(v) = std::env::var("MGS2_WALK_DIR")
        && auto.as_ref().is_some_and(|a| (a.from..a.from + a.seconds).contains(&time.elapsed_secs()))
    {
        let now = time.elapsed_secs();
        let seg = v.split(';').find(|seg| seg.split_once('@').is_none_or(|(_, t)| now < t.parse().unwrap_or(f32::MAX)));
        if let Some((x, z)) = seg.map(|seg| seg.split('@').next().unwrap_or("")).and_then(|d| d.split_once(',')) {
            dir = Vec2::new(x.parse().unwrap_or(0.0), z.parse().unwrap_or(0.0)).normalize_or_zero();
        }
    }
    let moving = dir != Vec2::ZERO;

    // Back to a wall: sidestep along it, Space knocks, away leaves it.
    if let Some(n) = p.wall {
        let away = Vec3::new(n.x, 0.0, n.y);
        // His right while facing away from the wall.
        let right3 = Vec3::new(n.y, 0.0, -n.x);
        if dir.dot(n) > 0.5 || p.prone || p.boxed || entering {
            p.wall = None;
        } else {
            body.yaw = n.x.atan2(n.y);
            // MGS2_KNOCK_AT=seconds: knock then (testing).
            static KNOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            let knock_test = std::env::var("MGS2_KNOCK_AT").ok().and_then(|v| v.parse::<f32>().ok()).is_some_and(|t| time.elapsed_secs() >= t)
                && !KNOCKED.swap(true, std::sync::atomic::Ordering::Relaxed);
            if keys.just_pressed(KeyCode::Space) || knock_test {
                body.play_once(if p.crouch { motion::WALL_CROUCH_KNOCK } else { motion::WALL_KNOCK });
                // The knock's sound and noise come from the motion's sound
                // event (snake.sar motion 67: sound 55 on the hand, which the
                // stage's tables make the wall's knock with noise 6).
                p.busy = 0.8;
                p.speed = 0.0;
                return;
            }
            let along = dir.dot(Vec2::new(right3.x, right3.z));
            let mut step = if along.abs() > 0.3 { along.signum() } else { 0.0 };
            let mut peek = 0.0;
            let mut next = body.pos + right3 * step * WALL_STEP_SPEED * dt;
            if let Some(col) = &game.collision {
                next = col.push_out(next, RADIUS, p.height(), STEP_UP.min(p.height() - 1.0));
                next = col.push_out_script(next, RADIUS, p.height(), STEP_UP.min(p.height() - 1.0));
                // Stop at the wall's end (nothing at his back) or a drop.
                let back = next + Vec3::Y * 800.0;
                let behind = col.ray(back, back - away * (RADIUS + 250.0));
                if behind.is_none() || col.floor_below(next.x, next.z, next.y + STEP_UP).is_none() {
                    next = body.pos;
                    // Still pushing at the wall's end: lean out and peek.
                    if behind.is_none() {
                        peek = step;
                    }
                    step = 0.0;
                }
            }
            body.pos = next;
            p.speed = if step != 0.0 { WALL_STEP_SPEED } else { 0.0 };
            if peek != 0.0 {
                // Peek motions: lean out (55 frames), then hold; standing
                // 61 / 62 to his right, 63 / 64 to his left (crouched +24).
                let base = if peek > 0.0 { motion::PEEK_RIGHT } else { motion::PEEK_LEFT } + if p.crouch { 24 } else { 0 };
                if body.motion == base + 1 {
                } else if body.motion == base && body.once && body.frame >= 54.0 {
                    body.play(base + 1);
                } else if body.motion != base {
                    body.play_once(base);
                }
                return;
            }
            body.play(match (p.crouch, step) {
                (false, s) if s > 0.0 => motion::WALL_RIGHT,
                (false, s) if s < 0.0 => motion::WALL_LEFT,
                (false, _) => motion::WALL,
                (true, s) if s > 0.0 => motion::WALL_CROUCH_RIGHT,
                (true, s) if s < 0.0 => motion::WALL_CROUCH_LEFT,
                (true, _) => motion::WALL_CROUCH,
            });
            return;
        }
    }

    let speed = match (moving, p.crouch, walking) {
        (false, _, _) => 0.0,
        (true, _, _) if p.prone => CRAWL_SPEED,
        (true, true, _) => CROUCH_SPEED,
        (true, false, true) => WALK_SPEED,
        (true, false, false) => RUN_SPEED,
    };
    p.speed = speed;
    if moving {
        body.yaw = turn_toward(body.yaw, dir.x.atan2(dir.y), TURN_RATE * dt);
    }

    // Horizontal move, then walls, then floor.
    let old = body.pos;
    let mut next = body.pos + Vec3::new(dir.x, 0.0, dir.y) * speed * dt;
    if let Some(col) = &game.collision {
        let wanted = next;
        next = col.push_out(next, RADIUS, p.height(), STEP_UP.min(p.height() - 1.0));
        next = col.push_out_script(next, RADIUS, p.height(), STEP_UP.min(p.height() - 1.0));
        // Walking into a wall for a moment puts his back to it.
        let push = Vec2::new(next.x - wanted.x, next.z - wanted.z);
        if moving && !p.prone && !p.boxed && !entering && push.length() > speed * dt * 0.5 && dir.dot(-push.normalize_or_zero()) > 0.8 {
            p.wall_push += dt;
            if p.wall_push >= WALL_PRESS {
                p.wall = Some(push.normalize_or_zero());
                p.wall_push = 0.0;
            }
        } else {
            p.wall_push = 0.0;
        }
        match col.floor_below(next.x, next.z, next.y + STEP_UP) {
            Some(floor) => {
                if next.y - floor > 30.0 {
                    // Off a ledge: the fall counts from here.
                    if p.fall_from.is_none() && p.fall_speed == 0.0 {
                        p.fall_from = Some(old.y);
                    }
                    p.fall_speed += GRAVITY * dt;
                    next.y = (next.y - p.fall_speed * dt).max(floor);
                } else {
                    next.y = floor;
                }
                if next.y <= floor {
                    p.fall_speed = 0.0;
                    if let Some(from) = p.fall_from.take() {
                        let drop = from - floor;
                        if drop >= FATAL_FALL {
                            let all = p.life.max(1);
                            p.damage(all);
                            crate::guard::noise(10, next);
                        } else if drop >= HARD_LANDING {
                            let life = p.life;
                            p.damage((drop / 100.0) as i32 - 20);
                            p.life = p.life.max(1).min(life);
                            crate::sfx::play(0x47, Some(next));
                            crate::guard::noise(6, next);
                        }
                    }
                }
            }
            // No ground: refuse the move (the edge of the walkable area).
            None => next = old,
        }
    }
    body.pos = next;
    if entering {
        let moved = Vec2::new(next.x - old.x, next.z - old.z).length();
        game.entrance -= moved;
        // Done, or walked into something.
        if game.entrance <= 0.0 || moved < speed * dt * 0.25 {
            game.entrance_done();
        }
    }

    let moved = Vec2::new(next.x - old.x, next.z - old.z).length();
    p.blocked = if speed > 0.0 && moved < speed * dt * 0.2 { p.blocked + dt } else { 0.0 };
    let speed = if p.blocked > 0.15 { 0.0 } else { speed };
    body.play(match (p.crouch, speed) {
        _ if p.prone && speed > 0.0 => motion::CRAWL,
        _ if p.prone => motion::PRONE_IDLE,
        (true, s) if s > 0.0 => motion::CROUCH_WALK,
        (true, _) => motion::CROUCH_IDLE,
        (false, s) if s >= RUN_SPEED => motion::RUN,
        (false, s) if s > 0.0 => motion::WALK,
        _ => motion::IDLE,
    });
}

/// Level walls are mostly single-sided; let camera rays hit them from
/// either side so the camera cannot slip behind one.
pub fn two_sided_rays(mut commands: Commands, parts: Query<Entity, Added<mgs2_bevy::scene::Part>>) {
    for e in &parts {
        commands.entity(e).insert(bevy::picking::mesh_picking::ray_cast::RayCastBackfaces);
    }
}

/// Orbiting follow camera: right mouse drag or Q/E to turn.
pub fn follow_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    player: Query<&Body, With<Player>>,
    guards: Query<&Body, (With<crate::guard::Guard>, Without<Player>)>,
    game: Res<Game>,
    mut cam: Query<(&mut Transform, &mut FollowCam)>,
    mut rays: MeshRayCast,
    parents: Query<&ChildOf>,
    bodies: Query<(), With<Body>>,
) {
    let Ok((mut tf, mut c)) = cam.single_mut() else { return };
    let b = match c.guard {
        Some(i) => guards.iter().nth(i),
        None => player.single().ok(),
    };
    let Some(b) = b else { return };
    let dt = time.delta_secs();
    if buttons.pressed(MouseButton::Right) {
        c.yaw -= motion.delta.x * 0.004;
        c.pitch = (c.pitch + motion.delta.y * 0.004).clamp(0.1, 1.55);
    }
    if keys.pressed(KeyCode::KeyQ) {
        c.yaw += 2.0 * dt;
    }
    if keys.pressed(KeyCode::KeyE) {
        c.yaw -= 2.0 * dt;
    }
    let target = b.pos * SCALE + Vec3::Y * 0.9;
    let manual = buttons.pressed(MouseButton::Right) || keys.pressed(KeyCode::KeyQ) || keys.pressed(KeyCode::KeyE);
    // MGS2_FOLLOW_CAM=1: start on the follow camera (testing).
    if keys.just_pressed(KeyCode::F3) || (std::env::var_os("MGS2_FOLLOW_CAM").is_some() && c.scripted && time.elapsed_secs() < 0.5) {
        c.scripted = !c.scripted;
    }
    // The stage's own camera for this area, eased into.
    if let (true, None, Some((_, sc))) = (c.scripted, c.guard, &game.cameras.active) {
        let (eye, look) = sc.place(b.pos + Vec3::Y * 900.0);
        let k = 1.0 - (-6.0 * dt).exp();
        tf.translation = tf.translation.lerp(eye * SCALE, k);
        c.look = c.look.lerp(look * SCALE, k);
        let look = c.look;
        tf.look_at(look, Vec3::Y);
        return;
    }
    c.look = target;
    let dir = |yaw: f32| Vec3::new(yaw.sin(), 0.0, yaw.cos()) * c.pitch.cos() + Vec3::Y * c.pitch.sin();
    let mut back = dir(c.yaw);
    let mut want = c.distance;
    if let Some(col) = &game.collision {
        // First obstacle between two points (metres) as a fraction: level
        // meshes (front faces: ceilings from below, walls facing Snake) and
        // the collision walls and floors.
        let not_character = |e: Entity| parents.get(e).map_or(true, |p| !bodies.contains(p.parent()));
        let settings = MeshRayCastSettings::default()
            .with_filter(&not_character)
            .with_visibility(RayCastVisibility::Any);
        let mut obstacle = |a: Vec3, b: Vec3| -> Option<f32> {
            let len = a.distance(b);
            let mesh = Dir3::new(b - a)
                .ok()
                .and_then(|d| rays.cast_ray(Ray3d::new(a, d), &settings).first().map(|(_, h)| h.distance / len))
                .filter(|t| *t <= 1.0);
            let wall = col.ray(a / SCALE, b / SCALE);
            mesh.into_iter().chain(wall).min_by(f32::total_cmp)
        };
        // Where the camera can be along `back`: under the ceiling, flattening
        // the view rather than closing in, then pulled in front of walls.
        let mut place = |back: Vec3| -> (Vec3, f32) {
            let mut cam = target + back * c.distance;
            if let Some(t) = obstacle(target, target + Vec3::Y * 20.0) {
                let ceiling = target.y + t * 20.0 - 0.3;
                cam.y = cam.y.min(ceiling.max(target.y));
            }
            let offset = cam - target;
            let mut d = offset.length();
            if let Some(t) = obstacle(target, cam) {
                d = (d * t - 0.3).max(0.25);
            }
            // Outside the playable area there are no walls to stop the
            // camera, so also require floor under it.
            let at = (target + offset.normalize_or(back) * d) / SCALE;
            if !col.floor_below(at.x, at.z, at.y).is_some_and(|y| y > target.y / SCALE - 3000.0) {
                d = d.min(0.8);
            }
            (offset.normalize_or(back), d)
        };
        (back, want) = place(back);
        // Boxed in (just through a door, in a corridor): swing round to the
        // nearest clear side.
        if !manual && want < c.distance * 0.5 {
            let best = [0.5f32, -0.5, 1.0, -1.0, 1.5, -1.5, 2.0, -2.0, 2.6, -2.6, std::f32::consts::PI]
                .into_iter()
                .find(|o| place(dir(c.yaw + o)).1 >= c.distance * 0.8);
            if let Some(o) = best {
                c.yaw += o.signum() * o.abs().min(2.5 * dt);
            }
        }
    }
    c.current = if want < c.current { want } else { c.current + (want - c.current) * (dt * 3.0).min(1.0) };
    tf.translation = target + back * c.current;
    tf.look_at(target, Vec3::Y);
}

/// A roll into a guard bowls him over (knocked out briefly).
pub fn roll_hits(snake: Query<(&Body, &Player)>, mut guards: Query<(&Body, &mut crate::guard::Guard), Without<Player>>) {
    let Ok((b, p)) = snake.single() else { return };
    if p.roll <= 0.0 {
        return;
    }
    let f = b.forward();
    for (gb, mut g) in &mut guards {
        let to = gb.pos - b.pos;
        if !g.asleep() && to.length() < 700.0 && to.normalize_or_zero().dot(f) > 0.3 && (to.y).abs() < 800.0 {
            crate::sfx::play(crate::sfx::BODY_FALL, Some(gb.pos));
            g.knock_out();
        }
    }
}
