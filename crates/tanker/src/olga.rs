//! Olga Gurlukovich, the boss on the navigational deck (w00b):
//! `chara 0x7798f6 <name> -m <model> -a <motions> -p x y z -d rx ry rz -v
//! <voice ids> -h <procs>...`.
//!
//! Her handler is `cmd_7798f6` in w00b's resident image (0x25EE10). Each
//! frame `FUN_0025EC50` calls the step function the pointer at actor+0xF84
//! points to; the pointer walks through step tables in the image (lists of
//! function addresses ending in a selector). This module runs those
//! tables, read from the disc, with Rust versions of the step functions:
//!
//! | step | does |
//! |---|---|
//! | 0x260168..0x260258 | set the timer (+0xFB0) to 179/119/59/29/14/9/5 frames |
//! | 0x260140 | count the timer down |
//! | 0x2615A0 | count down while aiming; being hit jumps to 0x1F6368 |
//! | 0x2617E8 | aim (motion 14) |
//! | 0x261640 | pick the aim point (`FUN_0025FD28`; here: Snake) |
//! | 0x261878 | shoot (motion 15) if she has rounds (+0xFB6) |
//! | 0x267FD8 / 0x268008 | wait for the motion to end |
//! | 0x260030 / 0x260100 | face the destination (+0x1030) / wait for the turn |
//! | 0x260A10 / 0x260B48 | run (motion 4) / walk (motion 7) |
//! | 0x260848 | move until within 800 mm of the destination |
//! | 0x262B00 / 0x262810 | reload (motion 12), 15 rounds |
//! | 0x2661E0 | down (motion 46) |
//! | 0x265CA0 | settle: idle, face the aim point |
//! | 0x267DE0 | choose the next sequence |
//!
//! Simplified: the selectors (`FUN_002679F0`, `FUN_00266448`,
//! `FUN_00266348`) and the cover choice (`FUN_002673B0`) depend on cover
//! objects and Snake's zone, which are not ported. Here the next sequence
//! alternates between moving to another of her positions (the table at
//! 0x1F5D80..) and one of her shoot-from-cover sequences, avoiding recent
//! picks as `FUN_00267DE0` does; she reloads when out of rounds.

use bevy::prelude::*;

use crate::Game;
use crate::body::{Body, turn_toward};
use crate::bullet::{self, Rng};
use crate::gcl::Spawn;
use crate::player::Player;

pub const OLGA_ACTOR: u32 = 0x7798F6;

/// `--boss-down-at`: take her stamina at this time (seconds), for tests.
#[derive(Resource)]
pub struct DownAt(pub f32);
/// Load address of a stage's resident image.
const RESIDENT_BASE: u32 = 0x001F_1480;

/// Step tables.
mod table {
    /// After her init (`FUN_00265E70`): move to the first destination.
    pub const START: u32 = 0x001F_62F0;
    /// Being hit.
    pub const HIT: u32 = 0x001F_6368;
    /// Down (stamina gone).
    pub const DOWN: u32 = 0x001F_6338;
    /// Reload, then move.
    pub const RELOAD: u32 = 0x001F_62A0;
    /// Move to the destination (run / walk).
    pub const MOVES: [u32; 2] = [0x001F_7DA8, 0x001F_7DC8];
    /// Shoot from cover: aim, then 2-3 shots with pauses.
    pub const ATTACKS: [u32; 6] = [0x001F_6410, 0x001F_6440, 0x001F_6480, 0x001F_64B0, 0x001F_64F0, 0x001F_6530];
}

/// Her positions (16-byte vectors in the image): crates and corners on her
/// side of the deck.
const POSITIONS: [u32; 7] = [0x001F_5D80, 0x001F_5D90, 0x001F_5DE0, 0x001F_5DF0, 0x001F_5E10, 0x001F_5E20, 0x001F_5E30];

const MAGAZINE: i32 = 15;
const RUN_SPEED: f32 = 3000.0;
const WALK_SPEED: f32 = 1300.0;
const TURN_RATE: f32 = 5.0;
/// Her stamina (+0xfb2, set to 0x80 at start).
pub const STAMINA: i32 = 0x80;
/// Dart damage by hit zone: her table at 0x1f6090 (eleven zones: head and
/// neck 32, the eight limb zones 8, the body 10; each entry's +0x1c >> 17).
pub const DART_HEAD: i32 = 32;
pub const DART_BODY: i32 = 10;
pub const DART_LIMB: i32 = 8;

#[derive(Component, Debug)]
pub struct Olga {
    pub stamina: i32,
    /// Step pointer (an address in the resident image).
    ptr: u32,
    timer: i32,
    rounds: i32,
    hit: bool,
    dest: Option<Vec3>,
    dest_at: Option<u32>,
    target_yaw: f32,
    speed: f32,
    /// Recent sequence picks, newest last.
    history: Vec<u32>,
    moved_last: bool,
    unknown: Vec<u32>,
    traced: u32,
    /// `-h` callback 1 (`#e5ee1f` in w00b): run when she is beaten; it
    /// ends the fight (camera, then the next cutscene).
    pub on_down: Option<u32>,
    /// Seconds since she went down; the callback runs once.
    pub down_for: f32,
    pub reported: bool,
    /// Reload sounds played so far (0..2).
    reload_sounds: u8,
}

impl Olga {
    pub fn new(on_down: Option<u32>) -> Self {
        Self {
            stamina: STAMINA,
            ptr: table::START,
            timer: 0,
            rounds: MAGAZINE,
            hit: false,
            dest: None,
            dest_at: None,
            target_yaw: 0.0,
            speed: 0.0,
            history: Vec::new(),
            moved_last: false,
            unknown: Vec::new(),
            traced: 0,
            on_down,
            reload_sounds: 0,
            down_for: 0.0,
            reported: false,
        }
    }

    /// A dart (`FUN_002680e0`): her stamina (+0xfb2) loses the zone's
    /// damage.
    pub fn hit(&mut self, damage: i32) {
        if self.down() {
            return;
        }
        self.stamina -= damage;
        self.hit = true;
        if self.stamina <= 0 {
            self.stamina = 0;
            self.ptr = table::DOWN;
        }
    }

    pub fn down(&self) -> bool {
        self.stamina <= 0
    }
}

/// The `-h` callback procs.
pub fn callbacks(s: &Spawn) -> Vec<u32> {
    s.opt(b'h').unwrap_or(&[]).iter().filter_map(|v| match v {
        crate::gcl::Val::Proc(p) => Some(*p),
        _ => None,
    }).collect()
}

/// (model hash, motion archive hash, position, facing in radians).
pub fn from_spawn(s: &Spawn) -> Option<(u32, u32, Vec3, f32)> {
    let model = s.opt(b'm')?.first()?.int()? as u32 & 0x00FF_FFFF;
    let motions = s.opt(b'a')?.first()?.int()? as u32 & 0x00FF_FFFF;
    let p = |i| s.opt_int(b'p', i).unwrap_or(0) as f32;
    let yaw = s.opt_int(b'd', 1).unwrap_or(0) as f32 / 4096.0 * std::f32::consts::TAU;
    Some((model, motions, Vec3::new(p(0), p(1), p(2)), yaw))
}

fn word(img: &[u8], addr: u32) -> Option<u32> {
    let o = addr.checked_sub(RESIDENT_BASE)? as usize;
    img.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn vec3(img: &[u8], addr: u32) -> Option<Vec3> {
    let f = |k: u32| word(img, addr + k * 4).map(f32::from_bits);
    Some(Vec3::new(f(0)?, f(1)?, f(2)?))
}

/// Motion length in frames (Olga's archive).
fn length(game: &Game, b: &Body, m: usize) -> f32 {
    b.motions.and_then(|s| game.data.motions[s].1.motions.get(m)).map_or(30.0, |x| x.length as f32)
}

pub fn think(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut rng: ResMut<Rng>,
    snake: Query<(&Body, &Player), Without<Olga>>,
    mut olgas: Query<(&mut Olga, &mut Body)>,
    trace: Option<Res<crate::guard::Trace>>,
    down_at: Option<Res<DownAt>>,
    mut clock: Local<f32>,
) {
    let Some(img) = game.data.resident.as_deref() else { return };
    let dt = time.delta_secs().min(0.05);
    *clock += dt * 60.0;
    let ticks = clock.floor() as i32;
    *clock -= ticks as f32;
    let Ok((sb, player)) = snake.single() else { return };
    let target = Target { feet: sb.pos, chest: sb.pos + Vec3::Y * (player.height() * 0.6), dead: player.dead() };
    for (mut o, mut b) in &mut olgas {
        if down_at.as_ref().is_some_and(|d| time.elapsed_secs() >= d.0) && !o.down() {
            o.stamina = 1;
            o.hit(1);
        }
        b.yaw = turn_toward(b.yaw, o.target_yaw, TURN_RATE * dt);
        // Run instant steps until one waits (bounded, as a safety net).
        for _ in 0..16 {
            let Some(f) = word(img, o.ptr) else {
                o.ptr = table::START;
                break;
            };
            let mut ctx = Ctx { img, game: &game, ticks, target, rng: &mut rng, commands: &mut commands };
            if !step(&mut o, &mut b, f, &mut ctx) {
                break;
            }
        }
        if trace.is_some() && o.ptr != o.traced {
            o.traced = o.ptr;
            eprintln!("olga: ptr {:#x} step {:#x} timer {} rounds {} pos ({:.0},{:.0},{:.0}) motion {}", o.ptr, word(img, o.ptr).unwrap_or(0), o.timer, o.rounds, b.pos.x, b.pos.y, b.pos.z, b.motion);
        }
        o.hit = false;
        if o.down() {
            o.down_for += dt;
        }
        if let (true, Some(d)) = (o.speed > 0.0, o.dest) {
            let to = Vec2::new(d.x - b.pos.x, d.z - b.pos.z);
            let v = to.normalize_or_zero() * (o.speed * dt).min(to.length());
            b.pos += Vec3::new(v.x, 0.0, v.y);
            o.target_yaw = to.x.atan2(to.y);
        }
    }
}

#[derive(Clone, Copy)]
struct Target {
    feet: Vec3,
    chest: Vec3,
    dead: bool,
}

struct Ctx<'a, 'w, 's> {
    img: &'a [u8],
    game: &'a Game,
    /// Whole 60 Hz frames since the last update (timers count these).
    ticks: i32,
    target: Target,
    rng: &'a mut Rng,
    commands: &'a mut Commands<'w, 's>,
}

/// Runs one step; returns true when the pointer moved on (another step may
/// run this frame), false when this step waits.
fn step(o: &mut Olga, b: &mut Body, f: u32, cx: &mut Ctx) -> bool {
    fn next(o: &mut Olga) -> bool {
        o.ptr += 4;
        true
    }
    fn face(o: &mut Olga, b: &Body, at: Vec3) {
        let d = at - b.pos;
        o.target_yaw = d.x.atan2(d.z);
    }
    let frames = cx.ticks;
    let motion_done = |b: &Body| b.once && b.frame >= length(cx.game, b, b.motion) - 1.0;
    match f {
        // Init (FUN_00265E70): idle, then the start table.
        0x0026_5E70 => {
            b.play(0);
            o.ptr = table::START;
            true
        }
        0x0026_0168 | 0x0026_0190 | 0x0026_01B8 | 0x0026_01E0 | 0x0026_0208 | 0x0026_0230 | 0x0026_0258 => {
            o.timer = match f {
                0x0026_0168 => 0xB3,
                0x0026_0190 => 0x77,
                0x0026_01B8 => 0x3B,
                0x0026_01E0 => 0x1D,
                0x0026_0208 => 0xE,
                0x0026_0230 => 9,
                _ => 5,
            };
            next(o)
        }
        0x0026_0140 | 0x0026_2BC8 => {
            o.timer -= frames;
            if o.timer < 1 { next(o) } else { false }
        }
        0x0026_15A0 => {
            if o.hit {
                o.ptr = table::HIT;
                return true;
            }
            face(o, b, cx.target.feet);
            o.timer -= frames;
            if o.timer < 1 { next(o) } else { false }
        }
        // Aim (motion 14).
        0x0026_17E8 | 0x0026_2D20 | 0x0026_2DF0 | 0x0026_2FF0 => {
            face(o, b, cx.target.feet);
            b.play(14);
            next(o)
        }
        0x0026_1640 => {
            face(o, b, cx.target.feet);
            next(o)
        }
        // Shoot (motion 15) if she has rounds.
        0x0026_1878 | 0x0026_3098 => {
            if o.rounds > 0 && !cx.target.dead {
                face(o, b, cx.target.feet);
                b.play_once(15);
                o.rounds -= 1;
                let muzzle = b.pos + Vec3::Y * 1400.0 + b.forward() * 400.0;
                bullet::fire(cx.commands, cx.rng, muzzle, cx.target.chest);
                crate::sfx::play(crate::sfx::USP, Some(b.pos));
            }
            next(o)
        }
        0x0026_7FD8 | 0x0026_8008 | 0x0026_30C0 => {
            if !b.once || motion_done(b) { next(o) } else { false }
        }
        0x0026_0030 => {
            if let Some(d) = o.dest {
                face(o, b, d);
            }
            next(o)
        }
        // Wait until turned (within 64 of 4096).
        0x0026_0100 => {
            use std::f32::consts::{PI, TAU};
            let diff = (o.target_yaw - b.yaw + PI).rem_euclid(TAU) - PI;
            if diff.abs() < 64.0 / 4096.0 * TAU { next(o) } else { false }
        }
        0x0026_0A10 => {
            b.play(4);
            o.speed = RUN_SPEED;
            next(o)
        }
        0x0026_0B48 => {
            b.play(7);
            o.speed = WALK_SPEED;
            next(o)
        }
        // Move until within 800 mm of the destination.
        0x0026_0848 | 0x0026_0DC0 | 0x0026_0CA0 => {
            let Some(d) = o.dest else { return next(o) };
            if Vec2::new(d.x - b.pos.x, d.z - b.pos.z).length() < 800.0 {
                b.pos = Vec3::new(d.x, b.pos.y, d.z);
                o.speed = 0.0;
                next(o)
            } else {
                o.speed = o.speed.max(WALK_SPEED);
                false
            }
        }
        // Reload.
        0x0026_2B00 => {
            b.play_once(12);
            o.reload_sounds = 0;
            next(o)
        }
        0x0026_2810 => {
            // FUN_00262810: magazine out (0x2f) at motion time 0x14f, in (9)
            // at 400 (the motion clock runs 5 a frame: frames 67 and 80).
            if b.frame >= 67.0 && o.reload_sounds < 1 {
                o.reload_sounds = 1;
                crate::sfx::play(0x2F, Some(b.pos));
            }
            if b.frame >= 80.0 && o.reload_sounds < 2 {
                o.reload_sounds = 2;
                crate::sfx::play(9, Some(b.pos));
            }
            if motion_done(b) {
                o.rounds = MAGAZINE;
                next(o)
            } else {
                false
            }
        }
        0x0026_61E0 => {
            b.play_once(46);
            next(o)
        }
        // The end of the down sequence: stays down.
        0x0026_6268 => false,
        // Settle at her position: idle, face Snake.
        0x0026_5CA0 | 0x0026_5E38 => {
            o.speed = 0.0;
            if !b.once || motion_done(b) {
                b.play(0);
            }
            face(o, b, cx.target.feet);
            next(o)
        }
        0x0026_7DE0 => {
            select(o, cx.img, cx.rng);
            true
        }
        _ => {
            if !o.unknown.contains(&f) {
                o.unknown.push(f);
                warn!("olga: step {f:#x} not ported; skipped");
            }
            next(o)
        }
    }
}

/// Stand-in for `FUN_00267DE0` and its selectors: reload when empty,
/// otherwise alternate between moving to another position and a shooting
/// sequence, avoiding recent picks.
fn select(o: &mut Olga, img: &[u8], rng: &mut Rng) {
    if o.down() {
        o.ptr = table::DOWN;
        return;
    }
    let pick = |rng: &mut Rng, from: &[u32], avoid: &[u32]| -> u32 {
        let fresh: Vec<u32> = from.iter().copied().filter(|t| !avoid.contains(t)).collect();
        let pool = if fresh.is_empty() { from.to_vec() } else { fresh };
        pool[rng.next() as usize % pool.len()]
    };
    let next = if o.rounds <= 0 {
        table::RELOAD
    } else if !o.moved_last && rng.next() % 2 == 0 {
        let at = pick(rng, &POSITIONS, &o.dest_at.into_iter().collect::<Vec<_>>());
        o.dest_at = Some(at);
        o.dest = vec3(img, at);
        o.moved_last = true;
        pick(rng, &table::MOVES, &[])
    } else {
        o.moved_last = false;
        let recent: Vec<u32> = o.history.iter().rev().take(3).copied().collect();
        pick(rng, &table::ATTACKS, &recent)
    };
    o.history.push(next);
    if o.history.len() > 8 {
        o.history.remove(0);
    }
    o.ptr = next;
}
