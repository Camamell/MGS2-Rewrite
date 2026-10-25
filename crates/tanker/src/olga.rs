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
//! | 0x265E70 | init: mode 1, idle, a first cover (`FUN_002673B0`) |
//! | 0x260168..0x260258, 0x260280 | set the timer (+0xFB0) to 179/119/59/29/14/9/5/1 frames |
//! | 0x260140 | count the timer down |
//! | 0x2615A0 | count down while aiming; being hit jumps to 0x1F6368 |
//! | 0x2617E8 | aim (motion 14) |
//! | 0x261640 | pick the aim point (`FUN_0025FD28`; here: Snake) |
//! | 0x261878 | shoot (motion 15) if she has rounds (+0xFB6), else a miss (+0xF7E) |
//! | 0x261A78 / 0x261710 | shoot from the lean (upper-body recoil), unless a wall is in her face |
//! | 0x264780.. 0x264F68 | lean out from the cover (0x1e / 0x1f / 0x22 / 0x23; the other side mirrored) |
//! | 0x2642E0.. 0x264668 | look out (0x30 / 0x31 / 0x34 / 0x35, mirrored) |
//! | 0x261938 / 0x2619D8 | the lean's aim (0x1a / 0x24) with a sight check |
//! | 0x261BD8.. 0x2620B8 | crouched at the cover: out, aim, back with a turn |
//! | 0x262158 / 0x2621F0 / 0x265080 | up over the low cover to aim (0x25 / 0x26) |
//! | 0x263EF8 / 0x263FA8 / 0x264088 | over the low cover: shots until the rounds run out |
//! | 0x25FF80 / 0x267FD8 / 0x268008 | wait for the motion to end |
//! | 0x261B48 | wait for the upper-body recoil to end |
//! | 0x260030 / 0x2600B0 / 0x2600D8 / 0x260100 | face the destination / the cover's way / its reverse; wait for the turn |
//! | 0x260A10 / 0x260B48 / 0x260C20 / 0x260E90 | run (4) / walk (7) / crouched walk (8) / run aiming (0x10) |
//! | 0x260FE0 / 0x261148 | side step along the cover (0x38 / 0x39) until level in z |
//! | 0x260848 | move until within 800 mm of the destination |
//! | 0x262B00 / 0x262A78 / 0x262810 | reload (motion 12), 15 rounds |
//! | 0x2661E0 | down (motion 46) |
//! | 0x265CA0 / 0x265E38 | settle: the stance's idle (+0xFAE), mirror off; face Snake or the cover's way |
//! | 0x267DE0 | choose the next sequence |
//!
//! The selectors are ported ([`select`]): `FUN_00267DE0` with its
//! eight-pick history and the game's draws, and by mode (+0xF70 & 3)
//! `FUN_002679F0` (where to go: a cover from [`Cover`], Snake's zone and
//! the hiding spots), `FUN_00266448` (attacks from the cover) and
//! `FUN_00266348` (looks out); [`sense`] keeps the fields they read.
//! Her taunts (+0x1066) are in: her six `-v` lines, lines 0 and 5 at her
//! points with the order 0x4000 and its event tables (0x1F80E8 /
//! 0x1F8110) and `-h` procs (the fight's camera events), lines 1-4 by
//! themselves in between ([`Requests`] carries the lines and procs to
//! the stage). Not ported (see `re/notes/guard-ai.md`): her other orders
//! (+0xF74) and the fixed points they send her to, her memory of Snake
//! (+0x1098, from messages), the other `-h` procs the selectors run, and
//! the upper-body motion layer (the recoil plays as a wait).

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
    /// Her init (`FUN_00265E70`).
    pub const INIT: u32 = 0x001F_6378;
    /// After the init: move to the first destination.
    pub const START: u32 = 0x001F_62F0;
    /// Being hit; also "settle and choose again".
    pub const HIT: u32 = 0x001F_6368;
    /// Down (stamina gone).
    pub const DOWN: u32 = 0x001F_6338;
    /// Move to the destination running / walking.
    pub const RUN: u32 = 0x001F_7DA8;
    pub const WALK: u32 = 0x001F_7DC8;
}

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
/// Snake's zone allows (narrowed under +0xf70 0x200000, stamina under
/// 0x26, or 0x40000), graded per cover (2 bits each, the first cover
/// highest; the second, the low box, never crouched), one picked with the
/// game's draws. Returns the cover, the side (0: spot +0x20, 1: +0x30)
/// and the spot; None when no side is allowed.
fn cover_choice(covers: &[Cover], img: &[u8], snake: Vec3, wish: u8, f70: u32, lcg: &mut Lcg) -> Option<(usize, usize, Vec2)> {
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
    if f70 & 0x20_0000 != 0 {
        allowed &= 0x3C;
    }
    if f70 & 0x4_0000 != 0 {
        allowed &= 0x3C;
    }
    let crouch = wish & 2 != 0;
    let graded: Vec<([Vec2; 2], u32)> = covers.iter().take(3).enumerate().map(|(i, c)| c.spots(s, crouch && i != 1)).collect();
    let grades = graded[0].1 << 4 | graded[1].1 << 2 | graded[2].1;
    let mut m = grades & allowed;
    if m == 0 {
        return None;
    }
    // A random subset of the candidates (AND with draws until empty), then
    // the last cover in it.
    loop {
        let next = m & lcg.draw();
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
        usize::from(lcg.draw() & 0x100 == 0)
    } else {
        usize::from(m & 1 == 0)
    };
    Some((i, side, graded[i].0[side]))
}

const MAGAZINE: i32 = 15;
const RUN_SPEED: f32 = 3000.0;
const WALK_SPEED: f32 = 1300.0;
const TURN_RATE: f32 = 5.0;
/// Her stamina (+0xfb2, set to 0x80 at start).
pub const STAMINA: i32 = 0x80;
// Dart damage by hit zone: her zone table at 0x1f6090 (head and heart 32,
// the eight limb zones 8, the body 10), read by `bullet::OlgaZones`.

/// The fixed points her orders send her to (+0x1030 compared with these
/// in `FUN_00266448`): no attack from them.
const FIXED_POINTS: [u32; 11] = [0x1F_5E00, 0x1F_5E10, 0x1F_5D60, 0x1F_5D70, 0x1F_5DE0, 0x1F_5DF0, 0x1F_5E20, 0x1F_5E30, 0x1F_6030, 0x1F_6040, 0x1F_5D50];

/// The game's random draws (`DAT_0019227c`).
#[derive(Clone, Copy, Debug)]
struct Lcg(u32);

impl Lcg {
    fn draw(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x5D58_8B65).wrapping_add(1);
        self.0
    }
}

/// A yaw in the game's 4096ths of a turn, as radians.
fn yaw(units: i32) -> f32 {
    (units & 0xFFF) as f32 / 4096.0 * std::f32::consts::TAU
}

#[derive(Component, Debug)]
pub struct Olga {
    pub stamina: i32,
    /// Step pointer (an address in the resident image).
    ptr: u32,
    /// +0xfb0.
    timer: i32,
    /// +0xfb6.
    rounds: i32,
    hit: bool,
    /// Her destination (+0x1030).
    dest: Option<Vec3>,
    /// The fixed point in the image the destination is, if one.
    dest_at: Option<u32>,
    target_yaw: f32,
    speed: f32,
    /// Moving without turning (a side step).
    strafe: bool,
    /// Frames left of the upper-body motion (layer 1: the shot's recoil).
    upper: Option<f32>,
    /// State flags (+0xf70): bits 0-1 the selector's mode, 0xc how
    /// cautious, 0x40 attacking, 0x80 leaning out, 0x100 a sight check
    /// asked for, 0x400 / 0x800 / 0x1000 at a low cover / on its +0x30 /
    /// +0x20 side, 0x8000 the same cover again, 0x100000.. stamina bands,
    /// 0x400000 in Snake's aim.
    f70: u32,
    /// Orders (+0xf74): not ported (her sense sets them; see the notes).
    f74: u32,
    /// Her zones in Snake's aim while she leans out (+0xf78).
    f78: u16,
    /// The sight check (+0xf7a): 1 clear to Snake's head, 0x8000 a wall
    /// within 1.5 m.
    f7a: u16,
    /// Attack picks since a move (+0xf7c).
    f7c: i16,
    /// Shots that could not be fired (+0xf7e).
    f7e: i16,
    /// Motion speed by stamina (+0xfac).
    fac: u8,
    /// The cover wish (+0xfae): bit 1 crouched, bit 0 the other stance;
    /// her idle motion.
    fae: u8,
    /// Attack picks (+0xfaf).
    faf: u8,
    /// The selector's last eight picks (+0xf88) and its ring index.
    history: [u32; 8],
    history_at: usize,
    lcg: Lcg,
    /// The cover she is at (+0x1084) and the one before (+0x1074).
    cover_at: Option<usize>,
    cover_was: Option<usize>,
    /// Frames exposed at her cover while attacking (+0x108c).
    exposed: i16,
    /// 256-frame periods since Snake's flag 0x80000000000 (+0x109a).
    quiet: u16,
    /// 256-frame periods of the fight, up to 8 (+0x1070).
    calm: i16,
    /// The frame clock (`DAT_001e6924`).
    frames: u32,
    unknown: Vec<u32>,
    traced: u32,
    /// `-h` callback 1 (`#e5ee1f` in w00b): run when she is beaten; it
    /// ends the fight (camera, then the next cutscene).
    pub on_down: Option<u32>,
    script: Script,
    /// Her taunts (+0x1066): the line reached (high nibble) and a count.
    taunt: u16,
    /// The line she is saying (+0xf0c).
    voice: Option<u32>,
    /// Seconds since she went down; the callback runs once.
    pub down_for: f32,
    pub reported: bool,
    /// Reload sounds played so far (0..2).
    reload_sounds: u8,
}

impl Olga {
    pub fn new(script: Script) -> Self {
        Self {
            stamina: STAMINA,
            ptr: table::INIT,
            timer: 0,
            rounds: MAGAZINE,
            hit: false,
            dest: None,
            dest_at: None,
            target_yaw: 0.0,
            speed: 0.0,
            strafe: false,
            upper: None,
            f70: 0,
            f74: 0,
            f78: 0,
            f7a: 0,
            f7c: 0,
            f7e: 0,
            fac: 0x40,
            fae: 0,
            faf: 0,
            history: [0; 8],
            history_at: 0,
            lcg: Lcg(0x2001_1113),
            cover_at: None,
            cover_was: None,
            exposed: 0,
            quiet: 0,
            calm: 0,
            frames: 0,
            unknown: Vec::new(),
            traced: 0,
            on_down: script.proc(1),
            script,
            taunt: 0,
            voice: None,
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

    /// `FUN_0025fe90`: the motion's mirror and upper-body layer off, flags
    /// `clear` (and 0x80000020) off and `set` on.
    fn reset(&mut self, b: &mut Body, set: u32, clear: u32) {
        b.mirror = false;
        self.upper = None;
        self.f70 = (self.f70 & !(clear | 0x8000_0020)) | set;
    }

    /// `FUN_002673b0` (with no order pending): takes cover from Snake.
    /// False (the game's 1) when no side is allowed; nothing changes then.
    fn take_cover(&mut self, cx: &Ctx, wish: u8) -> bool {
        // An order (+0xf74 beyond 0x170): a fixed point instead. Only the
        // taunt order 0x4000 is ported: point 0x1F5E70 + 16 x the line
        // reached, crouched from line 3 on.
        if self.f74 & !0x170 != 0 {
            self.fae = 0;
            self.f70 &= !0x9C00;
            if self.f74 & 0x4000 == 0 {
                return false;
            }
            let line = u32::from(self.taunt >> 12);
            if line > 2 {
                self.fae = 2;
            }
            let at = 0x001F_5E70 + 16 * line;
            info!("olga: to her point {at:#x} (line {line})");
            self.dest = point(cx.img, at);
            self.dest_at = Some(at);
            self.cover_was = self.cover_at;
            self.cover_at = None;
            self.exposed = 0;
            return true;
        }
        let Some((i, side, spot)) = cover_choice(&cx.game.covers, cx.img, cx.target.feet, wish, self.f70, &mut self.lcg) else {
            return false;
        };
        self.f70 &= !0x9C00;
        self.f70 |= if side == 0 { 0x1000 } else { 0x800 };
        if cx.game.covers[i].half.y * 2.0 < 1200.0 {
            self.f70 |= 0x400;
        }
        if self.cover_at == Some(i) {
            self.f70 |= 0x8000;
        }
        info!("olga: to cover {i} at ({:.0}, {:.0}), side {side}", spot.x, spot.y);
        self.cover_was = self.cover_at;
        self.cover_at = Some(i);
        self.exposed = 0;
        self.dest = Some(Vec3::new(spot.x, 0.0, spot.y));
        self.dest_at = None;
        true
    }
}

/// Her script data (`FUN_0025f230`): the `-h` procs (+0xe90 + 4n; 0 for
/// a value that is not a proc) and the `-v` voice lines (+0xef4, VOX ids).
#[derive(Clone, Debug, Default)]
pub struct Script {
    pub procs: Vec<u32>,
    pub voices: Vec<u32>,
}

impl Script {
    pub fn from_spawn(s: &Spawn) -> Self {
        let procs = s.opt(b'h').unwrap_or(&[]).iter().map(|v| match v {
            crate::gcl::Val::Proc(p) => *p,
            _ => 0,
        }).collect();
        let voices = (0..6).map_while(|i| s.opt_int(b'v', i)).map(|v| v as u32).collect();
        Self { procs, voices }
    }

    fn proc(&self, n: usize) -> Option<u32> {
        self.procs.get(n).copied().filter(|&p| p != 0)
    }
}

/// What her actor asks of the stage this frame (applied by
/// `main::olga_requests`): a voice line to play, script procs to run.
#[derive(Resource, Default)]
pub struct Requests {
    pub speech: Option<u32>,
    pub procs: Vec<u32>,
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

/// A point (16-byte vector) in the image.
fn point(img: &[u8], addr: u32) -> Option<Vec3> {
    let f = |k: u32| word(img, addr + k * 4).map(f32::from_bits);
    Some(Vec3::new(f(0)?, f(1)?, f(2)?))
}

/// `FUN_002685c8`: she says her `-v` line for the line count reached.
fn say(o: &mut Olga, requests: &mut Requests) {
    let n = usize::from(o.taunt >> 12);
    if let Some(&id) = o.script.voices.get(n) {
        info!("olga: line {n} ({id:#x})");
        requests.speech = Some(id);
        o.voice = Some(id);
    }
}

/// `FUN_002658c0`: once her line is over, the order is done: proc
/// `h[n]` runs, the next line is due and she settles (0x1F6368).
fn line_over(o: &mut Olga, requests: &mut Requests, n: usize) -> bool {
    if o.voice.is_some() {
        return false;
    }
    o.f74 = 0;
    o.f70 &= 0xF3FF_FFFF;
    requests.procs.extend(o.script.proc(n));
    o.ptr = table::HIT;
    o.taunt = o.taunt.wrapping_add(0x1000) & 0xF000;
    true
}

/// Entry `i` of a table of step tables.
fn entry(img: &[u8], base: u32, i: usize) -> u32 {
    word(img, base + 4 * i as u32).unwrap_or(0)
}

/// Motion length in frames (Olga's archive).
fn length(game: &Game, b: &Body, m: usize) -> f32 {
    b.motions.and_then(|s| game.data.motions[s].1.motions.get(m)).map_or(30.0, |x| x.length as f32)
}

/// One of her joints where the body stands now (model space turned and
/// placed).
fn joint(game: &Game, b: &Body, j: usize) -> Option<Vec3> {
    let model = &game.data.models.get(b.model)?.1;
    let motion = b.motions.and_then(|m| game.data.motions[m].1.motions.get(b.motion));
    let mats = mgs2_bevy::anim::pose_mirrored(model, motion, b.frame, false, None, b.mirror);
    mats.get(j).map(|m| b.pos + Quat::from_rotation_y(b.yaw) * Vec3::from(m.translation))
}

#[allow(clippy::too_many_arguments)]
pub fn think(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut rng: ResMut<Rng>,
    snake: Query<(&Body, &Player), Without<Olga>>,
    mut olgas: Query<(&mut Olga, &mut Body)>,
    trace: Option<Res<crate::guard::Trace>>,
    down_at: Option<Res<DownAt>>,
    mut requests: ResMut<Requests>,
    mut clock: Local<f32>,
) {
    let Some(img) = game.data.resident.as_deref() else { return };
    let dt = time.delta_secs().min(0.05);
    *clock += dt * 60.0;
    let ticks = clock.floor() as i32;
    *clock -= ticks as f32;
    let Ok((sb, player)) = snake.single() else { return };
    let target = Target {
        feet: sb.pos,
        chest: sb.pos + Vec3::Y * (player.height() * 0.6),
        head: sb.pos + Vec3::Y * (player.height() * 0.9),
        facing: sb.forward(),
        aiming: player.aiming,
        dead: player.dead(),
    };
    let zones = bullet::OlgaZones::read(img);
    let difficulty = game.banks.b16(crate::gcl::DIFFICULTY);
    for (mut o, mut b) in &mut olgas {
        if down_at.as_ref().is_some_and(|d| time.elapsed_secs() >= d.0) && !o.down() {
            o.stamina = 1;
            o.hit(1);
        }
        b.yaw = turn_toward(b.yaw, o.target_yaw, TURN_RATE * dt);
        if let Some(u) = o.upper.as_mut() {
            *u -= ticks as f32;
            if *u <= 0.0 {
                o.upper = None;
            }
        }
        // Her line is over once the stage no longer plays it.
        if let Some(v) = o.voice
            && requests.speech != Some(v)
            && game.speech.map(|s| s.0) != Some(v)
        {
            o.voice = None;
        }
        if !o.down() {
            sense(&mut o, &b, &game, target, zones.as_ref(), difficulty, ticks, &mut requests);
        }
        // Run instant steps until one waits (bounded, as a safety net).
        for _ in 0..16 {
            let Some(f) = word(img, o.ptr) else {
                o.ptr = table::START;
                break;
            };
            let mut ctx = Ctx { img, game: &game, ticks, target, difficulty, rng: &mut rng, commands: &mut commands, requests: &mut requests };
            if !step(&mut o, &mut b, f, &mut ctx) {
                break;
            }
        }
        if trace.is_some() && o.ptr != o.traced {
            o.traced = o.ptr;
            eprintln!(
                "olga: ptr {:#x} step {:#x} f70 {:#x} timer {} rounds {} f7e {} cover {:?} pos ({:.0},{:.0},{:.0}) motion {}{}",
                o.ptr,
                word(img, o.ptr).unwrap_or(0),
                o.f70,
                o.timer,
                o.rounds,
                o.f7e,
                o.cover_at,
                b.pos.x,
                b.pos.y,
                b.pos.z,
                b.motion,
                if b.mirror { " mirrored" } else { "" }
            );
        }
        o.hit = false;
        if o.down() {
            o.down_for += dt;
        }
        if let (true, Some(d)) = (o.speed > 0.0, o.dest) {
            let to = Vec2::new(d.x - b.pos.x, d.z - b.pos.z);
            let v = to.normalize_or_zero() * (o.speed * dt).min(to.length());
            b.pos += Vec3::new(v.x, 0.0, v.y);
            if !o.strafe {
                o.target_yaw = to.x.atan2(to.y);
            }
        }
    }
}

/// Her per-frame sense (`FUN_00269028`), the parts the selectors read:
/// the stamina bands, how cautious she is (+0xf70 & 0xc), Snake's aim on
/// her (`FUN_00268880`), the sight check and how long she has been
/// exposed at her cover. Not ported: her memory of Snake (+0x1098, set by
/// messages), the stage count (+0x1066) and the orders (+0xf74).
#[allow(clippy::too_many_arguments)]
fn sense(o: &mut Olga, b: &Body, game: &Game, target: Target, zones: Option<&bullet::OlgaZones>, difficulty: i16, ticks: i32, requests: &mut Requests) {
    let (band, fac) = match o.stamina {
        ..0x16 => (Some(0x30_0000), 0xC0),
        0x16..0x26 => (Some(0x20_0000), 0xC0),
        0x26..0x46 => (Some(0x10_0000), 0x80),
        _ => (None, 0x40),
    };
    if let Some(band) = band {
        o.f70 = (o.f70 & !0x30_0000) | band;
    }
    o.fac = fac;
    for _ in 0..ticks {
        o.frames = o.frames.wrapping_add(1);
        if o.frames & 0xFF == 0 {
            if o.calm < 8 {
                o.calm += 1;
            }
            // Snake's flag 0x80000000000 (DAT_001ecd40) resets this; what
            // sets it is not traced (taken as never).
            o.quiet = o.quiet.saturating_add(1);
        }
        // The taunt count: every 64 frames while lines remain and none
        // is being said.
        if o.frames & 0x3F == 0 && o.taunt >> 12 < 6 && o.voice.is_none() {
            o.taunt = o.taunt.wrapping_add(1);
        }
    }
    // Lines 1-4 come by themselves (not during motions 0x28..0x2f); lines
    // 0 and 5 wait for the order below.
    let line = o.taunt >> 12;
    if line < 6 && o.voice.is_none() && o.taunt & 0xFFF != 0 && !(0x28..=0x2F).contains(&b.motion) && line != 0 && line != 5 {
        say(o, requests);
        o.taunt = o.taunt.wrapping_add(0x1000) & 0xF000;
    }
    o.f70 &= !0xC;
    o.f70 |= match o.calm {
        ..2 => 0xC,
        2..4 => 8,
        _ if difficulty > 0x27 => 8,
        4..6 => 4,
        _ if difficulty > 0x1D => 4,
        _ => 0,
    };
    // Snake's aim (`FUN_00268880`, under his flags 0x800 and 1; flag 1 is
    // not known, taken as set): within 1400 mm of his line she knows it
    // (0x400000); leaning out, the zones the line meets (+0xf78).
    o.f70 &= !0x40_0000;
    if target.aiming {
        let eye = target.head;
        let to = b.pos + Vec3::Y * 1000.0 - eye;
        let along = to.dot(target.facing).max(0.0);
        if (to - target.facing * along).length() < 1400.0 {
            o.f70 |= 0x40_0000;
            if o.f70 & 0xC0080 == 0x80 {
                let far = eye + target.facing * (along + 2000.0);
                o.f78 = u16::from(zones.is_some_and(|z| z.crosses(game, b, eye, far)));
            }
        }
    }
    // The sight check, when a step asks for it: her head to Snake's.
    if o.f70 & 0x100 != 0 {
        o.f7a = 0;
        if let (Some(col), Some(head)) = (game.collision.as_ref(), joint(game, b, 12)) {
            o.f7a = match col.ray(head, target.head) {
                None => 1,
                Some(t) if t * head.distance(target.head) < 1500.0 => 0x8000,
                Some(_) => 0,
            };
        }
        o.f70 &= !0x100;
    }
    // Exposed at her cover while attacking: Snake now sees the spot
    // (`FUN_00268cf0` on both sides, 60 mm in hand).
    if o.f70 & 0x8_0200 != 0 {
        o.exposed = 0;
    } else if o.f70 & 0x40 != 0
        && let (Some(c), Some(d)) = (o.cover_at.and_then(|i| game.covers.get(i)), o.dest)
    {
        let s = Vec2::new(target.feet.x, target.feet.z);
        let k = c.corners();
        let x = k[2].x - if o.fae & 2 != 0 { 550.0 } else { 850.0 };
        let za = spot_z(300.0, x, k[0], k[2], s);
        let zb = || spot_z(-300.0, x, k[1], k[3], s);
        let exposed = if za == 0.0 {
            let zb = zb();
            zb == 0.0 || zb + 60.0 <= d.z
        } else if za - 60.0 < d.z {
            let zb = zb();
            zb != 0.0 && zb + 60.0 <= d.z
        } else {
            true
        };
        o.exposed = if exposed { o.exposed.saturating_add(ticks.max(1) as i16) } else { 0 };
    }
    // The orders (+0xf74), worked out again each frame. Ported: 0x4000,
    // a taunt line with its camera event (lines 0 and 5, once the count
    // reaches 8): she is sent back through her init to the line's point.
    if o.f70 & 0x20 == 0 && o.f74 & 0x1F0 == 0 {
        o.f74 &= 0x1_0000;
        if o.f70 & 0x800_0000 == 0 && o.taunt & 0xFFF >= 8 && matches!(o.taunt >> 12, 0 | 5) {
            o.f74 |= 0x4000;
            let at = 0x001F_5E70 + 16 * u32::from(o.taunt >> 12);
            if o.dest_at != Some(at) {
                o.ptr = table::INIT;
            }
        }
        if o.f74 & 0x200 == 0 {
            o.f74 &= !0x1_0000;
        }
    }
}

#[derive(Clone, Copy)]
struct Target {
    feet: Vec3,
    chest: Vec3,
    head: Vec3,
    facing: Vec3,
    aiming: bool,
    dead: bool,
}

struct Ctx<'a, 'w, 's> {
    img: &'a [u8],
    game: &'a Game,
    /// Whole 60 Hz frames since the last update (timers count these).
    ticks: i32,
    target: Target,
    difficulty: i16,
    rng: &'a mut Rng,
    commands: &'a mut Commands<'w, 's>,
    requests: &'a mut Requests,
}

/// A shot (+0xfcc = 1: her gun fires a round), from her gun hand toward
/// Snake's chest; `upper` is the upper-body recoil motion it plays.
fn shoot(o: &mut Olga, b: &Body, cx: &mut Ctx, upper: Option<usize>) {
    if o.rounds <= 0 || cx.target.dead {
        return;
    }
    o.rounds -= 1;
    // The right hand (joint 6); the left (10) in a mirrored motion.
    let hand = joint(cx.game, b, if b.mirror { 10 } else { 6 }).unwrap_or(b.pos + Vec3::Y * 1400.0);
    let muzzle = hand + (cx.target.chest - hand).normalize_or_zero() * 200.0;
    bullet::fire(cx.commands, cx.rng, muzzle, cx.target.chest);
    crate::sfx::play(crate::sfx::USP, Some(b.pos));
    if let Some(m) = upper {
        o.upper = Some(length(cx.game, b, m));
    }
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
        // Init (FUN_00265E70): idle, cover, then the start table.
        0x0026_5E70 => {
            o.reset(b, 0x201, 0x7000_03D2);
            o.dest = None;
            o.f78 = 0;
            o.f7a = 0;
            o.exposed = 0;
            b.play(o.fae as usize);
            o.take_cover(cx, o.fae);
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
        0x0026_0280 => {
            o.timer = 1;
            o.f70 |= 0x40;
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
        0x0026_1640 | 0x0025_FFE8 => {
            face(o, b, cx.target.feet);
            next(o)
        }
        // Shoot (motion 15) unless out of rounds or Snake is remembered
        // (0x80000); else the miss counts (+0xf7e).
        0x0026_1878 | 0x0026_3098 => {
            if o.f70 & 0x8_0000 == 0 && o.rounds > 0 {
                face(o, b, cx.target.feet);
                b.play_once(15);
                b.frame = 0.0;
                shoot(o, b, cx, None);
            } else {
                o.f7e += 1;
            }
            next(o)
        }
        // Shoot from the lean (upper-body motion 0x1b / 0xf), unless a wall
        // is right in front of her (+0xf7a 0x8000).
        0x0026_1A78 | 0x0026_1710 => {
            if o.f70 & 0x8_0000 == 0 && o.f7a & 0x8000 == 0 && o.rounds > 0 {
                if f == 0x0026_1710 {
                    face(o, b, cx.target.feet);
                }
                shoot(o, b, cx, Some(if f == 0x0026_1A78 { 0x1B } else { 0xF }));
                o.f70 = (o.f70 & 0x8FFF_FFFF) | 0xB000_0000;
            } else {
                o.f7e += 1;
            }
            next(o)
        }
        // The recoil over: the upper body back.
        0x0026_1B48 => {
            if o.upper.is_none() {
                o.f70 &= 0x7FFF_FFFF;
                next(o)
            } else {
                false
            }
        }
        // Over the low cover: up (0x19), then a shot each 9 frames
        // (motion 0x18) until her rounds run out (FUN_00264088 jumps back
        // to the shot while she has rounds and +0xf7e < 10).
        0x0026_3EF8 => {
            b.play_once(0x19);
            o.f70 = (o.f70 & 0x8FFF_FF7F) | 0x2_0110;
            next(o)
        }
        0x0026_3FA8 => {
            b.play_once(0x18);
            b.frame = 0.0;
            shoot(o, b, cx, None);
            next(o)
        }
        0x0026_4088 => {
            o.timer -= frames;
            if o.timer > 0 {
                return false;
            }
            if o.rounds > 0 && o.f7e < 10 {
                o.ptr -= 8;
            } else {
                o.ptr += 4;
            }
            true
        }
        // Wait for the motion to end (FUN_0025FF80 also breaks off when
        // Snake's aim is on her while remembered, 0xc0000).
        0x0025_FF80 => {
            if o.f70 & 0xC_0000 != 0 && o.f78 != 0 {
                o.ptr = table::HIT;
                o.f70 &= !0x80;
                return true;
            }
            if !b.once || motion_done(b) { next(o) } else { false }
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
        // Turn to the cover's facing (+0x68, 0xc00 for all three) or its
        // reverse.
        0x0026_00B0 => {
            o.target_yaw = yaw(0xC00);
            next(o)
        }
        0x0026_00D8 => {
            o.target_yaw = yaw(if o.cover_at.is_some() { 0xC00 - 0x800 } else { -0x800 });
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
        // Crouched walk (motion 8; speed assumed the walk's).
        0x0026_0C20 => {
            b.play(8);
            o.speed = WALK_SPEED;
            o.f70 &= 0x8FFF_7FFF;
            next(o)
        }
        // Run between covers aiming (motion 0x10; speed assumed the run's).
        0x0026_0E90 | 0x0026_0F38 => {
            o.timer = 0xF;
            b.play(0x10);
            o.speed = RUN_SPEED;
            o.f70 = (o.f70 & 0x8FFF_FF7F) | 0xD000_0100;
            next(o)
        }
        // Side step along the cover (0x38 / 0x39 by the side the
        // destination is on), until level with it in z.
        0x0026_0FE0 => {
            if let Some(d) = o.dest {
                let to = d - b.pos;
                let fwd = b.forward();
                b.play(if fwd.x * to.z - fwd.z * to.x > 0.0 { 0x38 } else { 0x39 });
                o.speed = WALK_SPEED;
                o.strafe = true;
            }
            o.f70 &= 0x8FFF_7FFF;
            next(o)
        }
        0x0026_1148 => {
            let Some(d) = o.dest else { return next(o) };
            if ((d.z - b.pos.z) * 0.015625) as i32 == 0 {
                b.pos.z = d.z;
                o.speed = 0.0;
                o.strafe = false;
                next(o)
            } else {
                false
            }
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
        // Lean out from the cover (motions 0x1e / 0x1f / 0x22 / 0x23; the
        // other side mirrored, model flags 0x300); some keep the arms in
        // the idle (an upper-body layer, not drawn here).
        0x0026_4780 | 0x0026_4820 | 0x0026_4860 | 0x0026_4900 | 0x0026_4BA0 | 0x0026_4C40 | 0x0026_4C80 | 0x0026_4D20 | 0x0026_4940
        | 0x0026_49C0 | 0x0026_4A10 | 0x0026_4A88 | 0x0026_4E40 | 0x0026_4EB8 | 0x0026_4EF0 | 0x0026_4F68 => {
            let (m, mirror) = match f {
                0x0026_4780 | 0x0026_4940 => (0x1E, false),
                0x0026_4820 | 0x0026_49C0 => (0x1E, true),
                0x0026_4860 | 0x0026_4A10 => (0x1F, false),
                0x0026_4900 | 0x0026_4A88 => (0x1F, true),
                0x0026_4BA0 | 0x0026_4E40 => (0x22, false),
                0x0026_4C40 | 0x0026_4EB8 => (0x22, true),
                0x0026_4C80 | 0x0026_4EF0 => (0x23, false),
                _ => (0x23, true),
            };
            b.play_once(m);
            b.mirror |= mirror;
            o.f70 = (o.f70 & 0x8FFF_FFFF) | 0x3000_0000;
            if matches!(f, 0x0026_4940 | 0x0026_49C0 | 0x0026_4A10 | 0x0026_4A88 | 0x0026_4E40 | 0x0026_4EB8 | 0x0026_4EF0 | 0x0026_4F68) {
                o.f70 |= 0x8000_0000;
            }
            next(o)
        }
        // Looks out from the cover (0x30 / 0x31 / 0x34 / 0x35, mirrored on
        // the other side).
        0x0026_42E0 | 0x0026_4360 | 0x0026_4398 | 0x0026_4418 | 0x0026_4530 | 0x0026_45B0 | 0x0026_45E8 | 0x0026_4668 => {
            let (m, mirror) = match f {
                0x0026_42E0 => (0x34, false),
                0x0026_4360 => (0x34, true),
                0x0026_4398 => (0x35, false),
                0x0026_4418 => (0x35, true),
                0x0026_4530 => (0x30, false),
                0x0026_45B0 => (0x30, true),
                0x0026_45E8 => (0x31, false),
                _ => (0x31, true),
            };
            b.play_once(m);
            b.mirror |= mirror;
            o.f70 &= 0x8FFF_FFFF;
            next(o)
        }
        // Crouched at the cover: out to the side (0x3c / 0x3a, mirrored on
        // the other side), the aim (0xe, a sight check; some with the legs
        // in motion 1 and the aim an upper-body layer, drawn whole here),
        // and back with a turn (0x3d / 0x3b).
        0x0026_1BD8 | 0x0026_1C60 | 0x0026_1CA0 | 0x0026_1D28 => {
            b.play_once(if matches!(f, 0x0026_1BD8 | 0x0026_1C60) { 0x3C } else { 0x3A });
            b.mirror |= matches!(f, 0x0026_1C60 | 0x0026_1D28);
            o.f70 &= 0xFFFF_FE7F;
            next(o)
        }
        0x0026_1F28 | 0x0026_2040 | 0x0026_1FA0 | 0x0026_20B8 => {
            b.play_once(0xE);
            o.f70 = (o.f70 & 0x8FFF_FFFF) | if matches!(f, 0x0026_1FA0 | 0x0026_20B8) { 0xB000_0100 } else { 0x3000_0100 };
            next(o)
        }
        0x0026_1D68 | 0x0026_1DD8 | 0x0026_1E48 | 0x0026_1EB8 => {
            let (m, turn) = match f {
                0x0026_1D68 => (0x3D, -0x7EB),
                0x0026_1DD8 => (0x3D, 0x753),
                0x0026_1E48 => (0x3B, -0x852),
                _ => (0x3B, 0x6D0),
            };
            b.play_once(m);
            b.yaw += yaw(turn);
            o.target_yaw = b.yaw;
            o.f70 &= 0x8FFF_FE7F;
            next(o)
        }
        // The aim from the lean (0x1a / 0x24), with a sight check.
        0x0026_1938 | 0x0026_19D8 => {
            b.play_once(if f == 0x0026_1938 { 0x1A } else { 0x24 });
            o.f70 = (o.f70 & 0x8FFF_FFFF) | 0x3000_0180;
            next(o)
        }
        // Over the low cover: up to aim (0x25 / 0x26), facing Snake.
        0x0026_2158 | 0x0026_21F0 | 0x0026_5080 => {
            face(o, b, cx.target.feet);
            b.play_once(if f == 0x0026_2158 { 0x25 } else { 0x26 });
            o.f70 = (o.f70 & 0x8FFF_FEFF) | if f == 0x0026_5080 { 0xB000_0080 } else { 0x3000_0080 };
            next(o)
        }
        // Motions played once.
        0x0026_4B30 | 0x0026_4DD0 | 0x0026_4AC0 | 0x0026_4D60 | 0x0026_4450 | 0x0026_44C0 | 0x0026_46A0 | 0x0026_4710 | 0x0026_5160 | 0x0026_2288 => {
            b.play_once(match f {
                0x0026_4B30 => 0x20,
                0x0026_4DD0 => 0x21,
                0x0026_4AC0 => 0x1A,
                0x0026_4D60 => 0x24,
                0x0026_4450 => 0x36,
                0x0026_44C0 => 0x37,
                0x0026_46A0 => 0x32,
                0x0026_4710 => 0x33,
                _ => 0x27,
            });
            next(o)
        }
        // Reload.
        0x0026_2B00 | 0x0026_2A78 => {
            if f == 0x0026_2A78 {
                o.f70 = (o.f70 & 0x8FFF_FFFF) | 0x40;
            }
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
        // Settle (FUN_00265CA0; 265E38 counts a miss first): the idle of
        // her stance (+0xfae: at a low cover the other stance), the
        // mirror and layer off; out of rounds the selector goes to
        // attack (which reloads); standing she faces Snake, crouched the
        // cover's way.
        0x0026_5CA0 | 0x0026_5E38 => {
            if f == 0x0026_5E38 && o.f70 & 0x8_0000 == 0 {
                o.f7e += 1;
            }
            if o.f70 & 0x400 != 0 {
                o.fae = (o.fae & !2) | 1;
            }
            o.reset(b, 0, 0x2_0250);
            b.play(o.fae as usize);
            if o.rounds < 1 {
                o.f70 = (o.f70 & 0xBFFF_FE7D) | 1;
            } else if o.f70 & 2 == 0 {
                o.f70 &= 0xBFFF_FE7F;
            }
            o.speed = 0.0;
            o.strafe = false;
            if o.fae & 2 == 0 {
                face(o, b, cx.target.feet);
            } else {
                o.target_yaw = yaw(0xC00);
                o.f70 &= 0x8FFF_FFFF;
            }
            next(o)
        }
        // The taunt events' steps: her line (FUN_00265890, 265A50), wait
        // for it to end (265998 also counts the timer down; 265980;
        // 265A98 holding motion 0x35 at frame 29), idle (265C20), the
        // lean of line 5 (0x35, 2657B0 mirrored).
        0x0026_5890 | 0x0026_5A50 => {
            say(o, cx.requests);
            next(o)
        }
        0x0026_5998 => {
            if line_over(o, cx.requests, 19) {
                return true;
            }
            o.timer -= frames;
            if o.timer < 1 { next(o) } else { false }
        }
        0x0026_5980 => line_over(o, cx.requests, 19),
        0x0026_5A98 => {
            b.play_once(0x35);
            b.frame = b.frame.max(29.0);
            line_over(o, cx.requests, 21)
        }
        0x0026_5C20 => {
            o.f70 |= 0x2_0000;
            b.play(0);
            next(o)
        }
        0x0026_5728 | 0x0026_57B0 => {
            b.play_once(0x35);
            b.mirror |= f == 0x0026_57B0;
            o.f70 = (o.f70 & 0x8FFF_FFFF) | 0x1_0000;
            next(o)
        }
        0x0026_7DE0 => select(o, b, cx),
        _ => {
            if !o.unknown.contains(&f) {
                o.unknown.push(f);
                warn!("olga: step {f:#x} not ported; skipped");
            }
            next(o)
        }
    }
}

/// What a selector gives `FUN_00267DE0`: a table taken as is (0), or
/// candidates to choose from (1).
enum Pick {
    Take(u32),
    Choose(Vec<u32>),
}

/// `FUN_00267DE0`: the selector for her mode (+0xf70 & 3) gives the next
/// table or candidates; of the candidates one not among her last eight
/// picks is drawn at random (and remembered), else the one picked longest
/// ago. Returns whether the pointer moved (an empty list leaves it, and
/// the selector runs again next frame).
fn select(o: &mut Olga, b: &Body, cx: &mut Ctx) -> bool {
    if o.down() {
        o.ptr = table::DOWN;
        return true;
    }
    let before = o.ptr;
    let pick = match o.f70 & 3 {
        0 => mode_move(o, b, cx),
        1 => mode_attack(o, cx),
        _ => mode_look(o, cx.img),
    };
    match pick {
        Pick::Take(t) => o.ptr = t,
        Pick::Choose(list) => {
            let mut oldest = 8;
            let mut fresh = 0u32;
            for (k, &t) in list.iter().enumerate() {
                match (0..8).rev().find(|&s| o.history[s] == t) {
                    Some(s) => {
                        let age = (s + 8 - o.history_at) & 7;
                        if age < oldest {
                            o.ptr = t;
                            oldest = age;
                        }
                    }
                    None => fresh |= 1 << k,
                }
            }
            if fresh != 0 {
                let mut m = fresh;
                let mut x = m & o.lcg.draw();
                while x != 0 {
                    m = x;
                    x = m & o.lcg.draw();
                }
                let t = list[m.trailing_zeros() as usize];
                o.ptr = t;
                o.history[o.history_at] = t;
                o.history_at = (o.history_at + 1) & 7;
            }
        }
    }
    o.ptr != before
}

/// `FUN_002679F0` (mode 0): where to go. Next mode attack (1), or on 3 of
/// 4 draws while she is not at her most cautious the looks (2); a cover
/// wish (two random bits; one while shots have failed) and a cover
/// (`FUN_002673b0`); the move by how the covers changed. Not ported: the
/// orders (+0xf74) and the stage count's (+0x1066) tables.
fn mode_move(o: &mut Olga, b: &Body, cx: &mut Ctx) -> Pick {
    if o.f70 & 0xC < 8 && o.lcg.draw() & 3 != 0 {
        o.f70 |= 0x202;
    } else {
        o.f70 |= 0x201;
    }
    o.f7c = 0;
    let r = o.lcg.draw();
    let mut wish = (if o.f7e != 0 { r & 1 } else { r & 3 }) as u8;
    if o.f74 & 0x70 != 0 {
        wish |= 2;
    }
    let t = if !o.take_cover(cx, wish) {
        let near = o.dest.is_some_and(|d| ((b.pos.x - d.x) / 100.0) as i32 == 0 || ((b.pos.z - d.z) / 100.0) as i32 == 0);
        if !near {
            return Pick::Take(table::HIT);
        }
        table::RUN
    } else if o.fae & 2 != 0 && wish & 2 != 0 && o.f70 & 0x8400 == 0x8000 {
        // The same high cover, crouched: a side step.
        0x001F_7E60
    } else if o.f70 & 0x8400 == 0x8400 {
        // The same low cover: the crouched walk.
        0x001F_7CB0
    } else if o.f70 & 0x20_0000 != 0 {
        table::RUN
    } else if o.f70 & 0x8_0000 == 0 && matches!((o.cover_was, o.cover_at), (Some(1), Some(0)) | (Some(0), Some(1)) | (Some(2), Some(0)) | (Some(2), Some(1))) {
        // Across to the other box: running and aiming.
        if o.f7e == 0 { 0x001F_7E30 } else { 0x001F_7E48 }
    } else if o.f70 & 0xC < 5 {
        table::RUN
    } else {
        table::WALK
    };
    o.fae = wish;
    Pick::Choose(vec![t])
}

/// `FUN_00266448` (mode 1): attacking from her cover. Out of rounds she
/// reloads; at a fixed point, with a wall in her face or exposed for 20
/// frames she settles and moves next; otherwise her cover's lean-out
/// attacks (by how cautious she is, the cover's side and her stance; over
/// a low cover the pop-up shots), or after misses the other ones. Every
/// fifth pick she goes to move (3 of 4) or look (mode 3). Not ported: the
/// orders (+0xf74) and Snake remembered (+0xf70 0x40000 / 0x80000).
fn mode_attack(o: &mut Olga, cx: &mut Ctx) -> Pick {
    let img = cx.img;
    let c = o.f70 & 0xC;
    o.f70 |= 0x40;
    o.faf = o.faf.wrapping_add(1);
    let pick = 'pick: {
        if o.rounds < 1 {
            o.f70 = (o.f70 & !0x180) | 0x40;
            break 'pick Pick::Take(entry(img, 0x001F_68E8, (o.fae & 3) as usize));
        }
        // Orders: the taunt events (h[18] / table 0x1F80E8 at line 0, h[20]
        // / 0x1F8110 at line 5); the others are not ported.
        if o.f74 & 0xFFFF != 0 {
            let event = match o.taunt >> 12 {
                0 => Some((18, 0x001F_80E8)),
                5 => Some((20, 0x001F_8110)),
                _ => None,
            };
            o.f70 = (o.f70 & 0x8FFF_FFFE) | 0x10;
            if let Some((h, t)) = event {
                info!("olga: taunt event at line {}", o.taunt >> 12);
                cx.requests.procs.extend(o.script.proc(h));
                break 'pick Pick::Take(t);
            }
            break 'pick Pick::Choose(Vec::new());
        }
        if o.dest_at.is_some_and(|a| FIXED_POINTS.contains(&a)) || o.f7a & 0x8000 != 0 || (o.exposed >= 0x14 && o.f70 & 0x8_0000 == 0) {
            o.f70 &= !1;
            break 'pick Pick::Take(table::HIT);
        }
        // Snake's memory bit (+0x1098 0x8000) is never set here.
        if c >= 4 && o.quiet >= 4 {
            o.f70 = (o.f70 & !1) | 0x2_0000;
        }
        let side = if o.f70 & 0x1000 != 0 {
            Some(0x001F_77E0)
        } else if o.f70 & 0x800 != 0 {
            Some(0x001F_7360)
        } else {
            None
        };
        let other = o.fae & 1 != 0;
        let mut list = Vec::new();
        if o.fae & 2 == 0 {
            if o.f7e == 0 {
                let i = (c >> 2) as usize;
                if o.rounds > 3 && o.f70 & 0x400 != 0 {
                    list.push(0x001F_7C80);
                }
                let mut j = i * 2;
                if o.f70 & 0x400 == 0 {
                    if other {
                        j += 8;
                    }
                } else {
                    list.push(entry(img, 0x001F_6810, i * 2));
                    list.push(entry(img, 0x001F_6810, i * 2 + 1));
                    if let Some(t) = side {
                        list.push(entry(img, t, i * 2));
                        list.push(entry(img, t, i * 2 + 1));
                    }
                    j += 8;
                }
                if let Some(t) = side {
                    list.push(entry(img, t, j));
                    list.push(entry(img, t, j + 1));
                }
            } else {
                if cx.difficulty < 0x1F && o.f7e < 2 {
                    o.f70 |= 0x2_0000;
                    if o.f70 & 0x1000 != 0 {
                        list.push(if other { 0x001F_79A0 } else { 0x001F_7920 });
                    } else if o.f70 & 0x800 != 0 {
                        list.push(if other { 0x001F_79E0 } else { 0x001F_7960 });
                    }
                } else {
                    if c > 4 || o.f7e & 1 == 0 {
                        o.f70 |= 0x2_0000;
                    }
                    if o.f70 & 0x1000 != 0 {
                        list.push(if other { 0x001F_78A0 } else { 0x001F_7820 });
                    } else if o.f70 & 0x800 != 0 {
                        list.push(if other { 0x001F_78E0 } else { 0x001F_7860 });
                    }
                }
                o.f7e -= 1;
            }
        } else {
            o.f70 &= !1;
            if o.f70 & 0x1000 != 0 {
                list.push(if other { 0x001F_7B90 } else { 0x001F_7B00 });
            } else if o.f70 & 0x800 != 0 {
                list.push(if other { 0x001F_7BD8 } else { 0x001F_7B48 });
            }
        }
        if o.faf >= 5 {
            o.faf = 0;
            if o.lcg.draw() & 3 == 0 {
                if o.f7c > 0 {
                    o.f70 &= !1;
                } else if o.f7e == 0 {
                    if c < 4 {
                        if o.lcg.draw() & 3 != 0 {
                            o.f70 |= 2;
                        }
                    } else if o.f70 & 0x40_0000 == 0 {
                        o.f70 |= 2;
                    }
                }
            } else {
                o.f70 &= !1;
            }
            o.f7c += 1;
        }
        Pick::Choose(list)
    };
    o.f78 = 0;
    o.f7a &= 0x7FFF;
    pick
}

/// `FUN_00266348` (modes 2 and 3): a look out from her cover (by its
/// side, her stance and Snake's aim on her), or at a low cover standing
/// the pop-up; mode 2 then attacks, mode 3 moves. (Her `-h` proc at
/// +0xeb0 runs here too: not ported.)
fn mode_look(o: &mut Olga, img: &[u8]) -> Pick {
    if o.f70 & 1 == 0 {
        o.f70 = (o.f70 & !2) | 0x41;
    } else {
        o.f70 &= !3;
    }
    let mut list = Vec::new();
    if o.f70 & 0x400 == 0 || o.fae != 0 {
        let base = if o.f70 & 0x1000 != 0 {
            Some(0x001F_6EA0)
        } else if o.f70 & 0x800 != 0 {
            Some(0x001F_6EE0)
        } else {
            None
        };
        if let Some(base) = base {
            list.push(entry(img, base, (o.fae & 3) as usize + if o.f70 & 0x40_0000 != 0 { 4 } else { 0 }));
        }
    } else {
        list.push(0x001F_6E70);
    }
    Pick::Choose(list)
}
