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
//! `FUN_00266348`) are not ported (their structure is in
//! `re/notes/guard-ai.md`). Here the next sequence alternates between a
//! move and one of her shoot-from-cover sequences, avoiding recent picks
//! as `FUN_00267DE0` does; she reloads when out of rounds. A move goes to
//! the cover spot `FUN_002673B0` would choose ([`Cover`], Snake's zone,
//! the hiding spots), else to one of her positions (0x1F5D80..).

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

/// One of her cover objects (`cmd_53209a`, w00b `FUN_00259f30`): the
/// centre (+0x40) and half-extents (+0x50) of its model's box, placed and
/// turned as the script says.
#[derive(Clone, Copy, Debug)]
pub struct Cover {
    pub centre: Vec3,
    pub half: Vec3,
}

impl Cover {
    /// From a model's box (`[min, max]`) and offset, placed at `at` and
    /// turned by `turn` (the game turns the box's two corners and halves
    /// their difference).
    pub fn from_box(b: [f32; 6], origin: [f32; 3], at: Vec3, turn: Quat) -> Self {
        let lo = turn * Vec3::new(b[0], b[1], b[2]);
        let hi = turn * Vec3::new(b[3], b[4], b[5]);
        Self { centre: (lo + hi) * 0.5 + Vec3::from(origin) + at, half: (hi - lo).abs() * 0.5 }
    }

    /// Its corners (x, z): +0x0, +0x8, +0x10, +0x18.
    fn corners(&self) -> [Vec2; 4] {
        let (c, h) = (self.centre, self.half);
        [Vec2::new(c.x + h.x, c.z - h.z), Vec2::new(c.x + h.x, c.z + h.z), Vec2::new(c.x - h.x, c.z - h.z), Vec2::new(c.x - h.x, c.z + h.z)]
    }

    /// `FUN_00268d80`: its two hiding spots (+0x20 / +0x30, on its -x side,
    /// 850 mm out or 550 crouched) and which of them hide her from `snake`
    /// (bit 1 / 2), the spots then kept on the deck.
    fn spots(&self, snake: Vec2, crouch: bool) -> ([Vec2; 2], u32) {
        let k = self.corners();
        let x = k[2].x - if crouch { 550.0 } else { 850.0 };
        let za = spot_z(300.0, x, k[0], k[2], snake);
        let zb = spot_z(-300.0, x, k[1], k[3], snake);
        let bits = u32::from(za != 0.0) | u32::from(zb != 0.0) << 1;
        ([on_deck(Vec2::new(x, za)), on_deck(Vec2::new(x, zb))], bits)
    }
}

/// The deck's bounds for her points (`FUN_00268a88`): z from -19150 up to
/// the slanted rail, (x + 12000) / 4 - 12850.
fn rail(x: f32) -> f32 {
    (x + 12000.0) * 0.25 - 12850.0
}

fn on_deck(p: Vec2) -> Vec2 {
    Vec2::new(p.x, p.y.max(-19150.0).min(rail(p.x)))
}

/// `FUN_00268af8`: on the line at `x`, where she is hidden behind the
/// edge a-b from `snake` (x, z): each ray from Snake through a corner,
/// moved `off` mm aside, crosses the line; the nearer crossing (min for
/// `off` > 0, else max) is kept, or the corner a's own z when `x` is
/// within 551 mm of b's and that is further out. 0 when off the deck.
fn spot_z(off: f32, x: f32, a: Vec2, b: Vec2, snake: Vec2) -> f32 {
    let cross = |c: Vec2| {
        let n = (c - snake).normalize_or_zero();
        (n.y * (x - snake.x) - off) / n.x + snake.y
    };
    let (za, zb) = (cross(a), cross(b));
    let z = if off > 0.0 { za.min(zb) } else { za.max(zb) };
    if !(-19150.0..=rail(x)).contains(&z) {
        0.0
    } else if ((x - b.x) / 551.0) as i32 == 0 {
        if off > 0.0 { z.max(a.y) } else { z.min(a.y) }
    } else {
        z
    }
}

/// Snake's zone (+0x109c, `FUN_00269028`): which side of nine lines he
/// is on, each through a cover corner and a crate corner from the table
/// at 0x3bb860 (bits 1..0x100 in this order).
fn zone(img: &[u8], snake: Vec2) -> u16 {
    const LINES: [(u32, u32); 9] = [
        (0x3B_B860, 0x3B_B8C0),
        (0x3B_B870, 0x3B_B8D0),
        (0x3B_B880, 0x3B_B8C0),
        (0x3B_B880, 0x3B_B8D0),
        (0x3B_B880, 0x3B_B8F8),
        (0x3B_B888, 0x3B_B8E8),
        (0x3B_B868, 0x3B_B8F8),
        (0x3B_B8A0, 0x3B_B868),
        (0x3B_B8B8, 0x3B_B8F8),
    ];
    let pt = |a: u32| Some(Vec2::new(f32::from_bits(word(img, a)?), f32::from_bits(word(img, a + 4)?)));
    LINES.iter().enumerate().fold(0, |z, (i, &(a, b))| match (pt(a), pt(b)) {
        (Some(c), Some(d)) if (snake.y - c.y) * (c.x - d.x) < (snake.x - c.x) * (c.y - d.y) => z | 1 << i,
        _ => z,
    })
}

/// `FUN_002673b0` with no order pending (+0xf74 clear): the cover sides
/// Snake's zone allows (narrowed while her stamina is under 0x26, +0xf70
/// 0x200000), graded per cover (2 bits each, the first cover highest; the
/// second, the low box, never crouched), one picked at random. Returns
/// the cover and the spot she goes to; None when no side is allowed.
fn cover_choice(covers: &[Cover], img: &[u8], snake: Vec3, crouch: bool, tired: bool, rng: &mut Rng) -> Option<(usize, Vec2)> {
    if covers.len() < 3 {
        return None;
    }
    let s = Vec2::new(snake.x, snake.z);
    let mut allowed = match zone(img, s) {
        0 => 5,
        0x20 if snake.z > -17400.0 => 0x18,
        0x20 => 5,
        0x60 => 0x30,
        0x160 => 0x32,
        0x170 => 0x1E,
        0x1F0 | 0x1F2 => 0x1C,
        0x1FA => 0x20,
        0x1FB => 0x22,
        0x1FF => 0x2A,
        _ => 0x3F,
    };
    if tired {
        allowed &= 0x3C;
    }
    let graded: Vec<([Vec2; 2], u32)> = covers.iter().take(3).enumerate().map(|(i, c)| c.spots(s, crouch && i != 1)).collect();
    let grades = graded[0].1 << 4 | graded[1].1 << 2 | graded[2].1;
    let mut m = grades & allowed;
    if m == 0 {
        return None;
    }
    // A random subset of the candidates (AND with draws until empty), then
    // the last cover in it.
    loop {
        let next = m & rng.next();
        if next == 0 {
            break;
        }
        m = next;
    }
    let mut i = 2;
    while m & 3 == 0 {
        m >>= 2;
        i -= 1;
    }
    // Both sides of it (and nothing further back): a coin flip.
    let side = if m == 3 {
        usize::from(rng.next() & 0x100 == 0)
    } else {
        usize::from(m & 1 == 0)
    };
    Some((i, graded[i].0[side]))
}

const MAGAZINE: i32 = 15;
const RUN_SPEED: f32 = 3000.0;
const WALK_SPEED: f32 = 1300.0;
const TURN_RATE: f32 = 5.0;
/// Her stamina (+0xfb2, set to 0x80 at start).
pub const STAMINA: i32 = 0x80;
// Dart damage by hit zone: her zone table at 0x1f6090 (head and heart 32,
// the eight limb zones 8, the body 10), read by `bullet::OlgaZones`.

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

pub fn word(img: &[u8], addr: u32) -> Option<u32> {
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
            select(o, cx.img, cx.rng, &cx.game.covers, cx.target.feet);
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
/// otherwise alternate between moving and a shooting sequence, avoiding
/// recent picks. A move goes where `FUN_002673b0` would send her: a cover
/// spot hidden from Snake (the wish's crouch bit drawn at random as in
/// `FUN_002679F0`), else one of her positions.
fn select(o: &mut Olga, img: &[u8], rng: &mut Rng, covers: &[Cover], snake: Vec3) {
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
        let crouch = rng.next() & 2 != 0;
        match cover_choice(covers, img, snake, crouch, o.stamina < 0x26, rng) {
            Some((i, spot)) => {
                info!("olga: to cover {i} at ({:.0}, {:.0})", spot.x, spot.y);
                o.dest_at = None;
                o.dest = Some(Vec3::new(spot.x, 0.0, spot.y));
            }
            None => {
                let at = pick(rng, &POSITIONS, &o.dest_at.into_iter().collect::<Vec<_>>());
                o.dest_at = Some(at);
                o.dest = vec3(img, at);
            }
        }
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
