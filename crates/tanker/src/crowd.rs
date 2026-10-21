//! The Marines standing in formation in the holds (`chara #b3e388`, w04c
//! resident `cmd_b3e388` / `FUN_0022a5d8`).
//!
//! `-a` lists the members, eleven values each: name, x y z, rotation x y z
//! (4096ths), variant (`% 22`), an unused value, idle variation (`% 6`) and
//! flags (0x10 / 0x20 / 0x40: rifle `m4a_nm`; 0x1 cap, 0x4 helmet, 0x80
//! hat, 0x100 headphones; 0x2 glasses). Variants below 18 are `us_def_1`
//! with some parts swapped for `w04a_us_def` parts (table at 0x1f73e0, 0x54
//! bytes per variant, one entry per part, -1 = keep); 19 is the commandant
//! (`sco_def`), 20-21 `us_def_pants`. Members placed at the origin are
//! walkers (`walkers`, `commandant`) and are left out here.
//!
//! Each stands looping his idle variation's `ushold` motion, with his
//! variant's parts, head accessories and slung rifle; the script's head
//! messages bring them to attention and turn their heads (`Head`). The
//! other formation modes (facing drills, looking about) are not done.

use bevy::prelude::*;
use mgs2_formats::kms;

use crate::gcl::{Spawn, Val};

pub const CROWD_ACTOR: u32 = 0xB3E388;
const RECORD: usize = 11;

#[derive(Component)]
pub struct Marine;

/// One member: feet position (mm), facing, variant, idle variation.
pub struct Member {
    pub pos: Vec3,
    pub yaw: f32,
    pub variant: i32,
    pub idle: i32,
    pub flags: i32,
}

/// The slung rifle (`m4a_nm`) of members with flag 0x10, 0x20 or 0x40:
/// `FUN_002517f8` hangs it on joint 2 at (-200, -250, -50) mm in that
/// joint's space, turned 2024/4096 about x (w04c tables 0x1f3880 /
/// 0x1f38b0).
pub const RIFLE_FLAGS: i32 = 0x70;
pub const RIFLE_JOINT: usize = 2;
pub const RIFLE_OFFSET: Vec3 = Vec3::new(-200.0, -250.0, -50.0);
pub fn rifle_turn() -> Quat {
    Quat::from_rotation_x(2024.0 / 4096.0 * std::f32::consts::TAU)
}

/// The head joint accessories hang on.
pub const HEAD: usize = 12;

/// Head accessories by flag bit (`FUN_0022a5d8`: cap 0x1 wins over helmet
/// 0x4, hat 0x80 and headphones 0x100; glasses 0x2 come on top).
pub fn accessories(flags: i32) -> Vec<&'static str> {
    let mut out = Vec::new();
    if flags & 0x185 != 0 {
        out.push(if flags & 1 != 0 {
            "obj_cap"
        } else if flags & 4 != 0 {
            "obj_helmet_def_s"
        } else if flags & 0x80 != 0 {
            "sco_hat"
        } else {
            "obj_headphone_def"
        });
    }
    if flags & 2 != 0 {
        out.push("obj_glass");
    }
    out
}

pub fn members(s: &Spawn) -> Vec<Member> {
    let Some(list) = s.opt(b'a') else { return Vec::new() };
    // `-s x y z`: the point the members face (`FUN_0022a5d8` turns each
    // toward it, `FUN_00223118`), overriding their own rotation.
    let face = (0..3).map(|i| s.opt_int(b's', i)).collect::<Option<Vec<i32>>>().map(|p| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32));
    list.chunks_exact(RECORD)
        .filter_map(|r| {
            let v = |i: usize| r[i].int().unwrap_or(0);
            if !matches!(r[0], Val::Name(_) | Val::Int(_)) {
                return None;
            }
            let pos = Vec3::new(v(1) as f32, v(2) as f32, v(3) as f32);
            let mut yaw = v(5) as f32 / 4096.0 * std::f32::consts::TAU;
            if let Some(f) = face {
                yaw = (f.x - pos.x).atan2(f.z - pos.z);
            }
            (pos.y != 0.0).then(|| Member {
                pos,
                yaw,
                variant: v(7).rem_euclid(22),
                idle: v(9).rem_euclid(6),
                flags: v(10),
            })
        })
        .collect()
}

/// `-c x y z dir member actor` (repeated): spotlights (`FUN_00230938`) at
/// `x y z`, worked by `member` and turned toward `actor` (w04c: two
/// Marines on the upper level light the commandant). The lights are not
/// drawn yet. Returns (member name, place, facing).
fn placed(s: &Spawn) -> Vec<(i32, Vec3, i32)> {
    let Some(list) = s.opt(b'c') else { return Vec::new() };
    list.chunks_exact(6)
        .map(|c| {
            let v = |i: usize| c[i].int().unwrap_or(0);
            (v(4), Vec3::new(v(0) as f32, v(1) as f32, v(2) as f32), v(3))
        })
        .collect()
}

/// Members placed at `0 0 n` walk route `n` of the stage (the holds' patrols:
/// one on Very Easy and Easy, two on Normal, three on Hard and Extreme in
/// w04a). Returns their routes. The commandant (variant 19, also placed at
/// the origin in w04c) is left out: see [`commandant`].
pub fn walkers(s: &Spawn) -> Vec<usize> {
    let Some(list) = s.opt(b'a') else { return Vec::new() };
    let placed = placed(s);
    list.chunks_exact(RECORD)
        .filter_map(|r| {
            let v = |i: usize| r[i].int().unwrap_or(0);
            let speaker = placed.iter().any(|p| p.0 == v(0));
            (v(1) == 0 && v(2) == 0 && v(3) >= 0 && v(7).rem_euclid(22) != 19 && !speaker).then_some(v(3) as usize)
        })
        .collect()
}

/// The commandant, a member of variant 19 placed at `0 0 n`: the crowd
/// makes him a walker on route `n` like the patrols (`FUN_002302a8`), with
/// flags | 0x21 (`FUN_0022a5d8`), which give him his own motions
/// (`guard::Anim::COMMANDANT`) and hide his rifle (`FUN_0022f9a8`). In
/// w04c he paces route 0 on the stage above the formation. Returns his
/// route and `-a` flags (his hat).
pub fn commandant(s: &Spawn) -> Option<(usize, i32)> {
    s.opt(b'a')?.chunks_exact(RECORD).find_map(|r| {
        let v = |i: usize| r[i].int().unwrap_or(0);
        (v(1) == 0 && v(2) == 0 && v(3) >= 0 && v(7).rem_euclid(22) == 19).then(|| (v(3) as usize, v(10)))
    })
}

/// The `-e` procs (by event: 0 a shot or explosion heard, 1 and 2 Snake
/// near the `-p` points, 3 a Marine heard something, 4 a Marine saw
/// Snake, ...). (`-t` is two level-of-detail distances, `FUN_00227fc8`.)
pub fn events(s: &Spawn) -> Vec<Option<u32>> {
    s
        .opt(b'e')
        .unwrap_or(&[])
        .iter()
        .map(|v| match v {
            Val::Proc(p) => Some(*p),
            _ => None,
        })
        .collect()
}

/// The `-p` points (up to two: x y z, rotation x y z, one more value):
/// Snake within 3 m of one, inside a cone along the point's facing, runs
/// event 1 or 2 once (`FUN_002286f0`). Returns (place mm, yaw, cos of the
/// cone's half angle within 1 m) for each.
pub fn points(s: &Spawn) -> Vec<(Vec3, f32, f32)> {
    let v = s.opt(b'p').unwrap_or(&[]);
    // The near cones (20 and 25 degrees, from the code); beyond 1 m both
    // are 15.
    let near = [0.939_692_6, 0.906_307_8];
    v.chunks_exact(7)
        .take(2)
        .zip(near)
        .map(|(r, c)| {
            let i = |k: usize| r[k].int().unwrap_or(0) as f32;
            (Vec3::new(i(0), i(1), i(2)), i(4) / 4096.0 * std::f32::consts::TAU, c)
        })
        .collect()
}

/// Whether Snake at `snake` sets off a `points` entry: within 3 m and
/// within the cone (horizontal) along its facing.
pub fn near_point(p: (Vec3, f32, f32), snake: Vec3) -> bool {
    let (at, yaw, near_cos) = p;
    let to = Vec2::new(snake.x - at.x, snake.z - at.z);
    let dist = snake.distance(at);
    if dist >= 3000.0 || to.length() < 1.0 {
        return false;
    }
    let cos = to.normalize().dot(Vec2::new(yaw.sin(), yaw.cos()));
    cos > if dist > 999.0 { 0.965_925_8 } else { near_cos }
}

/// Event index for "a Marine heard something" (`FUN_00224a58`).
pub const HEARD_SOMETHING: usize = 3;

/// Event index for "a Marine saw Snake" (`FUN_00224b80`, state bit 0x80).
pub const SAW_SNAKE: usize = 4;

/// Parts per entry of the variant table (`us_def_1` has 21 objects).
const PARTS: usize = 21;
/// Variants built from parts (the table has 18 per level of detail).
pub const PART_VARIANTS: i32 = 18;

/// Finds the variant table in a stage's resident image (it moves between
/// stages): entry 0 keeps every part, entry 1 swaps the neck and head (11,
/// 12) for library parts 0 and 1.
fn variant_table(resident: &[u8]) -> Option<usize> {
    let mut sig = Vec::with_capacity(PARTS * 8);
    for i in 0..PARTS * 2 {
        let v: i32 = match i {
            32 => 0,
            33 => 1,
            _ => -1,
        };
        sig.extend_from_slice(&v.to_le_bytes());
    }
    resident.windows(sig.len()).step_by(4).position(|w| w == sig.as_slice()).map(|p| p * 4)
}

/// Variant `v` (< 18) of a Marine: `us_def_1` with the parts the table
/// names taken from the part library `w04a_us_def` (`FUN_002bfb40`).
pub fn variant_model(base: &kms::Model, library: &kms::Model, resident: &[u8], v: i32) -> Option<kms::Model> {
    if !(0..PART_VARIANTS).contains(&v) {
        return None;
    }
    let table = variant_table(resident)? + v as usize * PARTS * 4;
    let mut m = base.clone();
    for (part, obj) in m.objects.iter_mut().enumerate().take(PARTS) {
        let o = table + part * 4;
        let j = i32::from_le_bytes(resident.get(o..o + 4)?.try_into().ok()?);
        if let Some(lib) = usize::try_from(j).ok().and_then(|j| library.objects.get(j)) {
            obj.meshes = lib.meshes.clone();
        }
    }
    Some(m)
}

/// Model for a variant: the base body for the marines, the commandant's
/// own model, the trousers-only variant.
pub fn model_name(variant: i32) -> &'static str {
    match variant {
        19 => "sco_def",
        20 | 21 => "us_def_pants",
        _ => "us_def_1",
    }
}

/// The formation's state, set by script messages to the crowd (the
/// handler before `FUN_00227ee0`): mode A (+0x8cab0) from messages 0, 7-15
/// and 18, a facing order (+0x8cab4) from message 5, and the head turn.
///
/// Mode 0 is at ease (each member's idle), 6-9 the facing drills (absolute
/// facings 0x400 / 0xc00 / 0 / 0x800, `FUN_00224a28`), 10-14 attention with
/// the head turned (+0x588 yaw -900 / 900 for 10 / 11, +0x58a pitch 800 /
/// -800 for 12 / 14, 13 leaves them; `FUN_00226338`), 15 looking about.
/// The head turn closes an eighth of the way each frame (`FUN_001419c8`)
/// and is applied to the head's matrix (`FUN_002282f0`). Angles in 4096ths.
#[derive(Default, Clone, Copy, Debug)]
pub struct Head {
    target: [f32; 2],
    now: [f32; 2],
    pub mode: u32,
    /// Message 5 (mode B 0x14): face 0.
    pub face_zero: bool,
}

pub const HEAD_JOINT: usize = 12;

/// `ushold` motions: attention (`FUN_00226280`), the drill's start
/// (`FUN_002265b8`), turns right / left / small, each followed by the next
/// motion as its end (`FUN_00226650`, `FUN_00226868`), small steps round
/// (`FUN_00225fc8`).
pub const ATTENTION: usize = 0x15;
const DRILL_START: usize = 0x1c;
const TURN_BIG_A: usize = 6;
const TURN_BIG_B: usize = 8;
const TURN_SMALL: usize = 10;
const STEP_A: usize = 0x1d;
const STEP_B: usize = 0x1e;

pub fn message(h: &mut Head, code: u32) {
    match code {
        0 => h.mode = 0,
        5 => h.face_zero = true,
        7..=15 => h.mode = code - 1,
        18 => h.mode = 15,
        _ => {}
    }
    match code {
        11 => h.target[0] = -900.0,
        12 => h.target[0] = 900.0,
        13 => h.target[1] = 800.0,
        15 => h.target[1] = -800.0,
        _ => {}
    }
}

impl Head {
    /// Advances `frames` frames; the head's turn, if any.
    pub fn step(&mut self, frames: f32) -> Option<(usize, Quat)> {
        let k = 1.0 - (7.0f32 / 8.0).powf(frames.min(10.0));
        for i in 0..2 {
            let d = self.target[i] - self.now[i];
            self.now[i] = if d.abs() < 1.0 { self.target[i] } else { self.now[i] + d * k };
        }
        if self.now == [0.0, 0.0] {
            return None;
        }
        let a = |v: f32| v / 4096.0 * std::f32::consts::TAU;
        Some((HEAD_JOINT, Quat::from_rotation_y(a(self.now[0])) * Quat::from_rotation_x(a(self.now[1]))))
    }
}

/// One member's place in the formation's state machine (`FUN_002273b8`:
/// action +0x5a8, sub-state +0x5aa, frame count +0x5ac).
#[derive(Component, Clone, Debug)]
pub struct Drill {
    pub idle: usize,
    action: u8,
    sub: u8,
    count: u32,
    /// The frame count at which the sub-state acts (+0x5b0).
    at: u32,
    /// Ordered facing (+0x584) and the yaw he turns to (+0x5ba), radians.
    facing: f32,
    target: f32,
    /// The turn motion playing.
    turn: usize,
}

impl Drill {
    pub fn new(idle: usize, yaw: f32) -> Self {
        Self { idle, action: 0, sub: 0, count: 0, at: 0, facing: yaw, target: yaw, turn: 0 }
    }
}

fn units(v: u32) -> f32 {
    v as f32 / 4096.0 * std::f32::consts::TAU
}

/// A facing the crowd code gives (4096ths) as a body yaw: its angles are
/// taken from the member toward the point behind him (`FUN_00223118` on
/// `pos - point`), so 0 faces -z, the way the formation looks; message 9
/// ("front") is 0 and 10 (about) 0x800.
fn game_yaw(v: u32) -> f32 {
    units(v) + std::f32::consts::PI
}

fn signed(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let d = a.rem_euclid(TAU);
    if d > PI { d - TAU } else { d }
}

fn set(d: &mut Drill, action: u8, sub: u8) {
    d.action = action;
    d.sub = sub;
    d.count = 0;
}

/// One 60 Hz frame of a member: the mode checks (`FUN_00225690`,
/// `FUN_002257f8`), then his action. `len` gives a motion's length.
pub fn drill(d: &mut Drill, h: &Head, body: &mut crate::body::Body, rng: &mut u32, len: impl Fn(usize) -> u32) {
    let mut rand = |n: u32| {
        *rng = rng.wrapping_mul(0x5d58_8b65).wrapping_add(1);
        (*rng >> 16) % n.max(1)
    };
    match h.mode {
        0 if d.action != 0 && d.action != 4 => {
            set(d, 0, 0);
            body.play(d.idle);
        }
        6..=9 => {
            let t = game_yaw([0x400, 0xc00, 0, 0x800][(h.mode - 6) as usize]);
            if d.action != 7 {
                d.facing = t;
                set(d, 7, 6);
            } else if signed(d.facing - t).abs() > 0.01 {
                d.facing = t;
                set(d, 7, 0);
            }
        }
        10..=14 if d.action != 6 => set(d, 6, 0),
        15 if d.action != 8 => set(d, 8, 1),
        _ => {}
    }
    if h.face_zero && d.action == 0 && signed(d.facing - game_yaw(0)).abs() > 0.01 {
        d.facing = game_yaw(0);
        set(d, 4, 0);
    }
    let count = d.count;
    match (d.action, d.sub) {
        // At ease.
        (0, _) => {
            body.play(d.idle);
            d.target = d.facing;
        }
        // Attention, then the head turn (`Head`).
        (6, 0) => {
            if count == 0 {
                d.at = rand(5);
            }
            if count == d.at {
                body.play_once(ATTENTION);
            }
            if count > d.at && count + 1 >= d.at + len(ATTENTION) {
                set(d, 6, 1);
                return;
            }
        }
        (6, _) => {}
        // The drill: brace, then turn.
        (7, 6) => {
            if count == 0 {
                d.at = rand(5);
            }
            if count == d.at {
                body.play_once(DRILL_START);
            }
            if count > d.at + 11 {
                set(d, 7, 0);
                return;
            }
        }
        (7, 0) => {
            if count == 0 {
                d.at = rand(5);
            }
            if count == d.at {
                let diff = signed(d.facing - body.yaw);
                let quarter = std::f32::consts::FRAC_PI_2 + 0.01;
                d.turn = if diff > quarter {
                    TURN_BIG_B
                } else if diff < -quarter {
                    TURN_BIG_A
                } else {
                    TURN_SMALL
                };
                body.play_once(d.turn);
                // (The game turns the big ones to a quarter short and lets
                // the motion carry the rest; here straight to the facing.)
                d.target = d.facing;
            }
            if count > d.at && count + 1 >= d.at + len(d.turn) {
                set(d, 7, 1);
                return;
            }
        }
        (7, _) => {
            if count == 0 {
                body.play_once(d.turn + 1);
            }
        }
        // Looking about (`FUN_00226400`): now and then a glance to one side
        // or the other of his facing.
        (8, _) => {
            if count == d.at {
                d.at = count + 60 + rand(120);
                let k = rand(5) as f32;
                d.target = d.facing + units(0x200) * (k - 2.0) * 0.5;
            }
        }
        // A step round to face 0 (`FUN_00225fc8`).
        (4, _) => {
            if count == 0 {
                d.at = rand(15);
                d.turn = 0;
            }
            if count == d.at {
                let diff = signed(d.facing - body.yaw);
                if diff.abs() > units(2) {
                    d.turn = if diff > 0.0 { STEP_B } else { STEP_A };
                    body.play_once(d.turn);
                }
            }
            if count == d.at + 15 {
                d.target = d.facing;
            }
            if count > d.at + 15 && (d.turn == 0 || count + 1 >= d.at + len(d.turn)) {
                set(d, 0, 0);
                body.play(d.idle);
                return;
            }
        }
        _ => {}
    }
    d.count += 1;
    // His yaw closes an eighth of the way each frame (+0x542).
    let diff = signed(d.target - body.yaw);
    body.yaw = if diff.abs() < 0.001 { d.target } else { body.yaw + diff / 8.0 };
}
