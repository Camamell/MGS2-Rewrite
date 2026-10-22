//! Gurlukovich soldiers: the `chara 0xf7f777` actor, ported from the game's
//! AI (w00a resident-image addresses; see `re/notes/guard-ai.md`).
//!
//! Faithful to the decompiled code:
//! - perception `FUN_002559C0`: near range `d` (`-i`), far range 1.5d,
//!   ±0x200 (45°) horizontally around the head direction and ±0x200
//!   vertically, line of sight; level 3 "seen" inside d, 2 "glimpse"
//!   between d and 1.5d, 1 "blocked"; a 250 x 1500 x 250 mm box around the
//!   guard counts as seen ("bump");
//! - suspicion meter `FUN_00256488` (0..0x400);
//! - normal-phase transitions `FUN_00227520`/`FUN_00227430`: meter > 0
//!   investigates, meter >= 2 or seen starts discovery, investigation
//!   escalates at meter > 8; investigation looks around for up to 300 frames;
//! - discovery `FUN_00238EB8`/`FUN_00237128`/`FUN_002381E8`: turn to face
//!   Snake during the "!" action, then set the meter to 0x400;
//! - alert brain `FUN_00229D30`: seen -> meter 0x400; the tracking flag
//!   expires 60 frames after the last sighting;
//! - phases (`Phase`) from the enemy manager `FUN_00220818`/`FUN_00220430`.
//!
//! - shooting (`FUN_00228710`/`FUN_00228F80`): aim 21 frames, a shot every
//!   4 frames with a 1-in-3 chance to end the burst, reload after 31 shots;
//!   bullets in `bullet.rs`.
//!
//! - close quarters: within 1 m and in sight, the rifle butt (motion 0x55;
//!   its damage is assumed).
//!
//! Simplified for now: radio reports, body stimuli (noises are
//! heard: `noise`; footprints and blood are followed), and the evasion search
//! pattern (guards visit nav nodes around the last known position).

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use mgs2_formats::hzx::Waypoint;

use crate::Game;
use crate::body::{Body, turn_toward};
use crate::bullet::{self, Rng};
use crate::gcl::Spawn;
use crate::player::{HEIGHT, Player, RADIUS, STEP_UP};

pub const GUARD_ACTOR: u32 = 0xF7F777;
/// The Deck-2 soldiers (w03a / w03b, `defender.c` `cmd_3259e5`): outside an
/// alert they run the watcher's brain (`FUN_0038b4f0` / `FUN_00392af8`;
/// the port's own, assumed the same) and take a new route by message 3;
/// in an alert they hold atcomm's defence points (see [`defend`]).
pub const DEFENDER_ACTOR: u32 = 0x3259E5;
/// "atcomm" (`atcomm.c` `cmd_c7280a`), which creates the defenders.
pub const ATCOMM_ACTOR: u32 = 0xC7280A;

/// atcomm's defence (init `FUN_00272b48` / `FUN_00272308`): the points its
/// defenders hold in an alert and what they defend.
#[derive(Clone, Debug, Default)]
pub struct Atcomm {
    /// `-d`: up to 16 points, x y z n.
    pub points: Vec<Vec3>,
    /// Each point's n (+0xc): which of [`Atcomm::patterns`] the attack
    /// from it runs (w03a: 6, the last 7).
    pub kinds: Vec<u8>,
    /// The attack patterns from the stage's resident image (w03a
    /// `PTR_DAT_001f50e0`): per point kind six step lists of (action,
    /// frames), each ending in action 0x1e. Empty: no attacks.
    pub patterns: Vec<[Vec<(u8, u8)>; 6]>,
    /// `-f` (+0x4a0): the place defended.
    pub front: Vec3,
    /// Path length from the front to each point (+0x450, at most 9 000 000).
    pub dist: Vec<f32>,
    /// `-t` 1 (+0x51c): each defender given point i at once
    /// (`FUN_00272308`) and the phase held at Alert while atcomm lives
    /// (`FUN_00272820`; w03b). See [`AtcommRun`].
    pub at_once: bool,
    /// `-p` (+0x504): the proc run once the defence is beaten (`-t` 1: no
    /// soldiers left to come and all of them down).
    pub cleared: Option<u32>,
    /// `-c` (+0x4e0): frames of the opening countdown (`-t` 1).
    pub countdown: u32,
    /// `-l`: how many lights (+0x4c0, bits of +0x2f3) stand over the
    /// defence (w03b's two searchlights); message 1 n puts light n out.
    pub lights: usize,
    /// Where a soldier down comes back in (`FUN_002732a0` ->
    /// `FUN_002158e8`): the manager's `-r` place +0x4e6 (0 unless message
    /// 2 changes it; the first).
    pub entry: Option<Vec3>,
}

/// atcomm's running state under `-t` 1 (w03b; `FUN_00272820` each frame,
/// `FUN_002727a8`, `FUN_002722a8`), and the manager's reinforcement counts
/// it reads (0x3e62b8 used, never handed out here, and 0x3e62ba left:
/// the manager's `-b`, default 16).
#[derive(Clone, Copy, Debug, Default)]
pub struct AtcommRun {
    started: bool,
    /// Soldiers still to come back in (0x3e62ba; each comeback takes one).
    pub left: i32,
    /// The mode (+0x2f0): 0 at first; 1 once fewer are left to come than
    /// there are defenders (the countdown is dropped); 2 once none are
    /// left (or a comeback held 3600 frames): all chase.
    pub mode: u8,
    /// The opening countdown (+0x4e0, from `-c`).
    countdown: u32,
    /// The attack gap (+0x4e2): when it runs out, every defender holding
    /// his point 60 frames or more starts an attack; it starts again at
    /// 180 frames (mode 0) or 12 (+0x4e2 set to 300 by a grenade).
    gap: u32,
    /// The gap ran out this frame.
    gap_out: bool,
    /// After the countdown: a 900-frame cycle (+0x4e8; its second half
    /// sends the first defender after Snake) and frames up to 300
    /// (+0x4ea; at 300 the third may throw a grenade).
    cycle: u32,
    warm: u32,
    /// Lights still on (+0x2f3 bits; their pattern 5 after the countdown).
    pub lights: u8,
    /// The `-p` proc has run.
    pub cleared: bool,
}

/// How far the front's path length to Snake is counted (+0x4b4, each
/// frame in `FUN_00272820`).
const FRONT_REACH: f32 = 120_000.0;

impl Atcomm {
    pub fn from_spawn(s: &Spawn, nav: Option<&crate::nav::Nav>) -> Option<Self> {
        let v = |letter, i| s.opt_int(letter, i).unwrap_or(0) as f32;
        let n = s.opt(b'd').map_or(0, |d| (d.len() / 4).min(16));
        let points: Vec<Vec3> = (0..n).map(|i| Vec3::new(v(b'd', 4 * i), v(b'd', 4 * i + 1), v(b'd', 4 * i + 2))).collect();
        let kinds = (0..n).map(|i| v(b'd', 4 * i + 3) as u8).collect();
        s.opt(b'f')?;
        let front = Vec3::new(v(b'f', 0), v(b'f', 1), v(b'f', 2));
        let dist = points.iter().map(|&p| nav.map_or(front.distance(p), |nav| nav.path_length(front, p, 9_000_000.0))).collect();
        let cleared = s.opt(b'p').and_then(|v| v.first()).and_then(|v| match v {
            crate::gcl::Val::Proc(p) => Some(*p),
            _ => None,
        });
        Some(Atcomm {
            points,
            kinds,
            patterns: Vec::new(),
            front,
            dist,
            at_once: s.opt_int(b't', 0) == Some(1),
            cleared,
            countdown: s.opt_int(b'c', 0).unwrap_or(0).max(0) as u32,
            lights: s.opt(b'l').map_or(0, |l| (l.len() / 3).min(2)),
            entry: None,
        })
    }

    /// Finds the attack pattern table in a resident image: its first list
    /// (kind 0, pattern 0) by its bytes, then the 8 x 6 pointers to the
    /// lists (w03a: the lists at 0x1f5020.., the table at 0x1f50e0; w03b
    /// has them elsewhere).
    pub fn read_patterns(img: &[u8]) -> Vec<[Vec<(u8, u8)>; 6]> {
        const BASE: usize = 0x001F_1480;
        const FIRST: [u8; 16] = [1, 6, 0xf, 0x12, 5, 0, 0x15, 1, 6, 0, 0x10, 0x12, 1, 6, 0x1e, 0];
        let u32_at = |o: usize| img.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
        let Some(first) = img.windows(FIRST.len()).step_by(2).position(|w| w == FIRST).map(|p| p * 2 + BASE) else {
            return Vec::new();
        };
        let Some(table) = (0..img.len() / 4).map(|i| i * 4).find(|&o| u32_at(o) == Some(first)) else {
            return Vec::new();
        };
        let list = |addr: usize| -> Option<Vec<(u8, u8)>> {
            let mut o = addr.checked_sub(BASE)?;
            let mut steps = Vec::new();
            while steps.len() < 32 {
                let (&a, &f) = (img.get(o)?, img.get(o + 1)?);
                steps.push((a, f));
                if a == 0x1e {
                    return Some(steps);
                }
                o += 2;
            }
            None
        };
        (0..8)
            .map_while(|k| {
                let lists: Option<Vec<_>> = (0..6).map(|p| list(u32_at(table + 4 * (k * 6 + p))?)).collect();
                lists.and_then(|l| <[Vec<(u8, u8)>; 6]>::try_from(l).ok())
            })
            .collect()
    }
}

/// The defenders' actions (`FUN_0028bd90`, into +0xfe4): the motion, and
/// whether it plays once (`FUN_0028bb68`, which marks its end in +0xf58)
/// rather than looping (`FUN_0028bca0` / `FUN_0028ba78`). Actions 0x15 and
/// up shoot (`FUN_0028cdf8`). Motions with bits 0xc0000000 (0xb, 0xc,
/// 0x18) are played mirrored by the game (model flags 0x300, the muzzle
/// then on joint +0xca0): not done here; w03a's points never use them.
/// 0x1a is 0x154 for soldiers with +0xf2c bit 0x4000, else 0x44 (taken).
fn defend_action(action: u8) -> Option<(usize, bool)> {
    Some(match action {
        1 => (0, false),
        2 => (0x45, false),
        3 => (0x47, true),
        4 => (0x46, true),
        5 => (0x107, true),
        6 => (0x108, true),
        7 | 0xb => (0x10a, true),
        8 | 0xc => (0x109, true),
        9 => (0x10f, true),
        10 => (0x110, true),
        0xd => (99, false),
        0xe => (0x4b, false),
        0xf => (0x41, false),
        0x10 => (0x40, false),
        0x11 => (0x3c, false),
        0x12 => (0x4e, true),
        0x13 => (0x15, false),
        0x15 => (0x10c, false),
        0x16 | 0x18 => (0x10b, false),
        0x17 => (0x111, false),
        0x19 | 0x1b => (0x3d, false),
        0x1a => (0x44, false),
        0x1c => (0x10d, false),
        0x1d => (0x10e, false),
        _ => return None,
    })
}

/// A defender's part in the defence (his unit, soldier +0x430).
#[derive(Clone, Copy, Debug, Default)]
pub struct Defend {
    /// `-t` (+0x330f) 2 or 3: a point of his own, held (w03b; not ported).
    pub fixed: bool,
    /// Has joined this alert's defence (`FUN_00272ed0`).
    joined: bool,
    /// The point he holds (+0xb) and whether he is at it (sub-state 9
    /// stance 4) rather than on his way (sub-state 10, stances 0x13 / 0x14).
    point: Option<usize>,
    holding: bool,
    /// Frames in the stance (+0x338).
    frames: u32,
    /// Has left the defence for the chase this alert (+0x12 bit 0x10).
    chase: bool,
    /// His place in atcomm's squad (+0x13c): under `-t` 2 / 3 his point.
    pub index: usize,
    /// Frames down (asleep or knocked out; `-t` 2 / 3 under `-t` 1 atcomm).
    down: u32,
    /// Came back in to stand where he is (sub-state 10 stance 4,
    /// `FUN_0028d8a8`) rather than run to his point.
    stand: bool,
    /// Came back in: first motion 0x15 (stance 0x3f, `FUN_0028c200`).
    entering: bool,
    /// Where he stands holding it (the point, or where walls stopped him).
    spot: Vec3,
    /// The attack from the point (stance 0x1b), when running.
    attack: Option<PointAttack>,
}

/// An attack from the point (`FUN_0028c640` starts it, `FUN_0028cdf8`
/// runs it): a pattern's steps one after another.
#[derive(Clone, Copy, Debug, Default)]
struct PointAttack {
    /// The point's kind (+8) and the pattern (+0x16).
    kind: usize,
    pattern: usize,
    /// The step (+9), its frames (+0x1a) and the stance's frames (+0x338).
    step: usize,
    count: u32,
    frames: u32,
    /// Frames before he may fire again (+0x3a8 +0x20).
    cooldown: u32,
    /// Has fired in this step (for the log).
    fired: bool,
}

/// The fire window of a shooting step (`FUN_0028dfc8`: +0x17 12, +0x18
/// 48, +0x19 90): shots after 12 frames for 48, every 6 frames; a step
/// of length 1 lasts the whole 150.
const ATTACK_DELAY: u32 = 12;
const ATTACK_FIRE: u32 = 48;
const ATTACK_AFTER: u32 = 90;
/// The line of fire's masks (`FUN_0028a448`: `FUN_0014f230` mode 0xf,
/// walls 0x10000, floors 0x20, each with 0x40000 / 0x80000 added as for
/// the photos' sight line).
const FIRE_WALLS: u32 = 0x10000 | 0x40000 | 0x80000;
const FIRE_FLOORS: u32 = 0x20 | 0x40000;
/// A reinforcement trooper (`cmd_cc9a07`), created by the squad actor
/// `0x5bb4d4` that the manager's `-a` proc makes when the alert starts.
pub const TROOPER_ACTOR: u32 = 0xCC9A07;
/// The enemy manager (`cmd_ddf5ca`): phases and their timers.
pub const MANAGER_ACTOR: u32 = 0xDDF5CA;

/// The soldier's radio (`radio.c`, model `rad`), made by his body setup
/// `FUN_0025a718` for every model but 0xc9aaaf / 0x296d9c / 0xd7aaaf /
/// 0xfbbaaf. Each frame `FUN_00265fe0` hangs it on a joint of his model
/// (objects are 0x130 bytes after a 0xc0 header) by his radio flags
/// (+0xb4): 0x2 the left hand (joint 10, place 0x1f3e00), 0x4 the right
/// hand (joint 6, 0x1f3e20), else the belt (joint 0, 0x1f3e40 / 0x1f3e50:
/// offset in mm, then turns in 4096ths about x, y, z by `FUN_001253f8`).
/// The belt and the left hand are used (flag 0x2 is set by the radio
/// motions during a call, `FUN_0025ebc0` / `FUN_0025f068`); the right
/// hand (wounded left arm) is not. The order of the three turns is assumed
/// (`EulerRot::XYZ`).
pub mod radio {
    use bevy::math::{EulerRot, Quat, Vec3};
    use bevy::prelude::*;

    pub const JOINT: usize = 0;
    pub const OFFSET: Vec3 = Vec3::new(-120.0, 25.0, -97.5);
    pub fn turn() -> Quat {
        euler(4028, 3777, 2048)
    }
    /// The left hand: joint 10, 0x1f3e00 / 0x1f3e10.
    pub const HAND_JOINT: usize = 10;
    pub const HAND_OFFSET: Vec3 = Vec3::new(-5.0, -77.5, -67.5);
    pub fn hand_turn() -> Quat {
        euler(993, 193, -1038)
    }
    fn euler(x: i16, y: i16, z: i16) -> Quat {
        let a = |n: i16| f32::from(n) / 4096.0 * std::f32::consts::TAU;
        Quat::from_euler(EulerRot::XYZ, a(x), a(y), a(z))
    }

    /// Marks a soldier's radio model.
    #[derive(Component)]
    pub struct Radio;

    /// Hangs each radio on the belt or in the hand by its owner's flag.
    pub fn place(guards: Query<&super::Guard>, mut radios: Query<&mut crate::body::Held, With<Radio>>) {
        for mut h in &mut radios {
            let Ok(g) = guards.get(h.owner) else { continue };
            // A shot radio is not drawn (`FUN_00265fe0`: +0x164 bit 0x80000).
            h.hidden = g.radio_shot;
            let (object, offset) = if g.radio_in_hand { (HAND_JOINT, HAND_OFFSET) } else { (JOINT, OFFSET) };
            if h.object != object {
                h.object = object;
                h.offset = offset;
                h.grip = if g.radio_in_hand { hand_turn() } else { turn() };
            }
        }
    }
}

mod motion {
    pub const IDLE: usize = 0;
    pub const WALK: usize = 1;
    pub const RUN: usize = 2;
    /// Rifle raised, aiming.
    pub const AIM: usize = 4;
    /// The "!": action 2 of the action table `FUN_002240f8`, motion
    /// `FUN_0025da40` (0x5c; 0xec / 0x165 under flags 8 / 0x4000).
    pub const EXCLAIM: usize = 92;
    /// Looking around (action 5 -> motion 16, 440 frames), as the
    /// investigation's sub-state 10 does.
    pub const LOOK: usize = 16;
}

const WALK_SPEED: f32 = 1330.0;
const RUN_SPEED: f32 = 4050.0;
/// The Caution walk (gait 5, `FUN_0025dfd8`): motion 99 (0x63; 0x158 /
/// 300 / 0xf9 under body flags 0x4000 / 0x40 / 8, not used here), whose
/// root track goes 960 mm in its 54 frames (as the walk's 2110 mm in 95
/// and the run's 2160 mm in 32 give the speeds above).
const CAUTION_WALK: usize = 99;
const CAUTION_WALK_SPEED: f32 = 960.0 / 54.0 * 60.0;

/// The gait (+0xf5a) for the leg to his next route point, picked on
/// arrival at a point from its action and flags (`FUN_00224c28`): for the
/// waiting actions (below 0x1d) 1 with look flags 0x2 / 0x8, else 5 in
/// Caution, else 0 (10 under `-s` bit 0x40: no Tanker soldier); action
/// 0x1d 2, 0x1e 3, 0x25 7, 0x26 9.
fn pick_gait(wp: &mgs2_formats::hzx::Waypoint, action: u16, phase: &Phase) -> u8 {
    let raw = if wp.flags & 0xC0 != 0 { 0 } else { POINT_ACTIONS.get(wp.action.max(0) as usize).copied().unwrap_or(0) };
    match raw {
        0x1D => 2,
        0x1E => 3,
        0x25 => 7,
        0x26 => 9,
        _ if action < 0x1D && wp.flags & 0xA != 0 => 1,
        _ if phase.kind == PhaseKind::Caution => 5,
        _ => 0,
    }
}

/// A gait's motion, speed (mm/s) and the body's turn from the way he goes
/// (`FUN_0025dfd8`; the forms under body flags 8 / 0x40 / 0x4000 are not
/// used here). Speeds by the motions' root tracks, as the walk's (2110 mm
/// in 95 frames) and the run's (2160 in 32) give the port's: gait 1 motion
/// 0x12 (1150 mm in 53), 5 [`CAUTION_WALK`], 2 / 3 the side steps 0x65 /
/// 0x66 (740 mm in 90, facing the way less / plus 0x400), 7 the run
/// (assumed: `FUN_0025daa0`'s motion). Gait 9 (motion 0x3c) is not used in
/// the Tanker: the walk. Soldiers only (the Marines keep their walk).
fn gait_motion(g: &Guard, gait: u8) -> (usize, f32, f32) {
    if g.anim.check.is_none() {
        return (g.anim.walk, g.anim.walk_speed, 0.0);
    }
    let side = 740.0 / 90.0 * 60.0;
    match gait {
        1 => (0x12, 1150.0 / 53.0 * 60.0, 0.0),
        2 => (0x65, side, -std::f32::consts::FRAC_PI_2),
        3 => (0x66, side, std::f32::consts::FRAC_PI_2),
        5 => (CAUTION_WALK, CAUTION_WALK_SPEED, 0.0),
        7 => (g.anim.run, RUN_SPEED, 0.0),
        _ => (g.anim.walk, g.anim.walk_speed, 0.0),
    }
}

/// Whether a route point's action leaves him blind on frame `t` of its
/// motion (body flag 0x1000, which `FUN_002559c0` reads as nothing seen):
/// the step `FUN_00223980` of actions 4, 5, 0xe, 0x12-0x14 and 0x18 sets
/// it on every frame; `FUN_00260320` (actions 6 and 0xc) on frames 0x3d
/// to 0x1e6 (with 0x200000000000000), the dozing part of motion 17.
fn point_blind(action: u16, t: u32) -> bool {
    match action {
        4 | 5 | 0xE | 0x12..=0x14 | 0x18 => true,
        6 | 0xC => (0x3D..0x3D + 0x1AA).contains(&t),
        _ => false,
    }
}

/// A route point's action and wait (`FUN_00224870`): in Caution (Bs16:0x112
/// == 3) flags 0x800 make the action 2, flags 0x1000 and actions 4-6 make
/// it 0xd (the head swing), and then a wait of -1 is 180 frames.
fn point_plan(wp: &mgs2_formats::hzx::Waypoint, phase: &Phase) -> (u16, i16) {
    let action = point_action(wp);
    if phase.kind != PhaseKind::Caution {
        return (action, wp.wait);
    }
    let caution = if wp.flags & 0x1000 != 0 || (4..=6).contains(&action) {
        Some(0xD)
    } else if wp.flags & 0x800 != 0 {
        Some(2)
    } else {
        None
    };
    match caution {
        Some(a) => (a, if wp.wait == -1 { 180 } else { wp.wait }),
        None => (action, wp.wait),
    }
}
const TURN_RATE: f32 = TAU;
/// Eye height above the feet (head joint position, approximate).
const EYE: f32 = 1600.0;
/// Frames a soldier looks a box over before lifting it (assumed).
const BOX_CHECK_FRAMES: u32 = 90;

/// 4096 units per turn.
const ANGLE: f32 = TAU / 4096.0;
/// A footprint is looked at this far above the floor (as a target this
/// tall; the sight line goes to 60%), so the floor itself does not hide it.
const PRINT_SIGHT: f32 = 100.0;
/// Frames the "!" reaction lasts when its motion cannot be read; otherwise
/// it lasts the motion (action 2 plays motion 92 once, `FUN_0025e9b8`, and
/// the discovery state waits for it to finish, `FUN_00237128`).
const EXCLAIM_FRAMES: u32 = 45;
/// A discovery whose "!" is already over.
const EXCLAIM_DONE: u32 = 1 << 20;
pub const METER_MAX: i16 = 0x400;

/// Global phase, `Bs16:0x112`, as run by the enemy manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PhaseKind {
    #[default]
    Infiltration = 0,
    Alert = 1,
    Evasion = 2,
    Caution = 3,
}

#[derive(Resource, Debug)]
pub struct Phase {
    pub kind: PhaseKind,
    /// Manager +0x1C: follows the highest guard meter, decays 16/frame.
    pub meter: i32,
    /// Evasion countdown (manager +0x38), frames.
    pub evasion: i32,
    /// Caution countdown (manager +0x24), frames.
    pub caution: i32,
    /// `-h` of the manager actor (default 0xE10 = 60 s).
    pub caution_time: i32,
    pub last_seen: Option<Vec3>,
    changed: bool,
    /// Frames until an explosion (noise 11) raises the meter
    /// (`FUN_0021ff20`: 60 frames, then `FUN_00220008` feeds 0x400).
    blast: u32,
    /// The manager's spoken radio lines (`-v`, `-y`).
    pub radio: Radio,
    /// Voice clips to start (taken by `main::radio_lines`).
    pub say: Vec<u32>,
    /// A clip to stop: its speaker was cut off.
    pub stop: Option<u32>,
    /// The manager's "no answer" slots (+0x17c / +0x19c): (soldier,
    /// frames left).
    no_answer: Vec<(usize, i32)>,
    /// The priority of the soldier line on the air and its frames left
    /// (`DAT_003fef7c`, which `FUN_0021d230` reads, -1 when none; the count
    /// `DAT_003fef80` clears it).
    on_air: Option<(u32, u32)>,
    /// Where soldiers are who have called this discovery in (controller
    /// +0x344 bit 8, set as sub-states 3 / 0x15 go to 0xe), this frame.
    callers: Vec<Vec3>,
    /// The manager's `-c` point (0x3fea80; the first of up to eight, as
    /// `FUN_0021eeb0` reads it at index 0x3fea74, which only init sets): a
    /// soldier whose radio is dead runs there to report. Without `-c` the
    /// ELF's zero vector 0x190a30 (the origin).
    pub report_at: Vec3,
    /// The radios are jammed (`DAT_0017f48c` bit 0x10, set while chaff
    /// (`bomb_chaff.c`) is in the air): the port's chaff timer.
    pub jammed: bool,
    /// atcomm under `-t` 1 (w03b).
    pub atc: AtcommRun,
    /// Frames since a soldier last saw Snake (the squad's +0x214, which
    /// `FUN_00275af8` counts up and zeroes on a new sighting; read by the
    /// defenders, `FUN_0028c640` / `FUN_0028c8e8`).
    pub unseen: u32,
    /// The manager's `-e`: (route, point, proc) run when a soldier reaches
    /// that point (0x3fe828, `FUN_002217f0` / `FUN_0021dc60`).
    route_events: Vec<(usize, usize, u32)>,
    /// The manager's `-f`: (route, point, clip, frames) said there by a
    /// soldier whose point waits -4 (0x401e48, `FUN_00221858`: the length
    /// / 5).
    route_lines: Vec<(usize, usize, u32, u32)>,
}

/// The soldiers' radio lines, as the enemy manager's init reads them
/// (w00a `FUN_00221970`'s options `v` and `y`, both `str[n]` data blocks of
/// little-endian words):
/// - `-v`: (voice clip `0x20000000 + sector`, length in 1/300 s) pairs up
///   to a negative id, kept at 0x401648 / 0x400ca8 (length / 5 = frames);
/// - `-y`: HQ's replies (0x4015f0) up to a negative word; -2 then a clip
///   for 0x4014a8, -3 then one for 0x401638.
///
/// `FUN_0021cdb8(line, soldier)` plays line `line & 0xfff` of the soldier's
/// set: + 0x38 per voice (controller +0x284), + 0xe0 for soldiers with body
/// flag +0xf2c & 0x4000 (w00a's table has 8 sets of 0x38: voices 4-7 say
/// the same lines in other words); with bit 0x8000000 the length is the
/// table's, else 0xf0 frames. Unless bit 0x40000, effect 99 (the radio
/// click) plays first; bit 0x4000000 pans it from the soldier. A report
/// (0x1c00000b, `FUN_0021cb40`) queues HQ's reply `0x2000000 | n` (n =
/// manager message 3, default 0) for when it ends.
#[derive(Clone, Debug, Default)]
pub struct Radio {
    pub lines: Vec<(u32, u32)>,
    pub replies: Vec<u32>,
    /// `-y`'s clip after -2 (0x4014a8; w00a "We've lost communication
    /// with the aft deck.").
    pub lost: Option<u32>,
    /// `-x`: HQ's answers per area (0x4014b0: a count and up to four
    /// clips each; the data lists clips, -2 between areas, -1 at the end).
    pub areas: Vec<Vec<u32>>,
}

/// The areas' trap names, in the order `FUN_0021c670` tests them, with the
/// `-x` entry each one answers from (the last four reuse entries 8-11).
const AREA_TRAPS: [(u32, usize); 16] = [
    (0x3A30EC, 0),
    (0x3A30ED, 1),
    (0x3A30EE, 2),
    (0x3A30EF, 3),
    (0x3A30F0, 4),
    (0x3A30F1, 5),
    (0x3A30F2, 6),
    (0x3A30F3, 7),
    (0x3A30F4, 8),
    (0x3A30F5, 9),
    (0x461DD7, 10),
    (0x461DD8, 11),
    (0x461DD9, 8),
    (0x461DDA, 9),
    (0x461DDB, 10),
    (0x461DDC, 11),
];
/// HQ's answer when Snake is in none of the areas (0x10816004: line 0x1c4,
/// w00a "Acknowledged. Assault team, check it out.").
const HQ_CHECK_IT_OUT: u32 = 0x1081_6004;

/// A soldier's report on the radio (`FUN_0021cdb8(0x1c00000b)`).
pub const LINE_REPORT: u32 = 0x1C00_000B;
/// "It was nothing" (0xc000012, a comrade up again in Infiltration).
pub const LINE_ALL_CLEAR: u32 = 0x0C00_0012;
/// "Is there somebody there?" / "Over there?" (investigation, state 6).
pub const LINE_SOMEBODY: u32 = 0x0C04_001D;
pub const LINE_OVER_THERE: u32 = 0x0C04_001E;
/// The discovery's calls for backup: "It's the enemy! I need help!"
/// (0x2c000004, after the "!") and "Possible hostile sighted. Send
/// backup." (0x2c000000, after a shot heard).
pub const LINE_ENEMY: u32 = 0x2C00_0004;
pub const LINE_SIGHTED: u32 = 0x2C00_0000;
/// "What was that sound?" (normal state 3, a second noise) and its
/// Evasion form.
const LINE_SOUND: u32 = 0x0C04_0013;
const LINE_SOUND_EVASION: u32 = 0x0C04_0014;
/// "What's going on!?": normal state 3's line when the noise was a
/// comrade's "!" (level 8).
const LINE_WHATS_GOING: u32 = 0x0C04_0020;
/// Lines per voice in the `-v` table.
const LINES_PER_VOICE: u32 = 0x38;
/// The radio click (`FUN_00114b30(0x20, 0x3f, 99)`).
const RADIO_CLICK: u32 = 99;
/// A "no answer" slot's length (frames).
const NO_ANSWER_FRAMES: i32 = 900;
/// HQ's lines to a silent soldier: "What's going on? Respond!" (+0x1c0
/// block, line 0xa) and the lost contact (`-y`'s -2 clip; when no soldier
/// is still able, `FUN_00221098`, the game says 0x914100 instead of
/// 0x914000, the same clip without the follow-up 0x1081400e "Carry out an
/// investigation immediately.", which the port leaves out: its condition,
/// +0xf28 bit 0x200000000, is not traced).
const HQ_RESPOND: u32 = 0x1081_400A;
const HQ_LOST: u32 = 0x0091_4000;

impl Radio {
    pub fn from_script(spawn: &Spawn, script: &mgs2_formats::gcx::Gcx) -> Self {
        let words = |letter| -> Vec<i32> {
            spawn
                .opt_int(letter, 0)
                .and_then(|n| script.resources.get(n as usize))
                .map(|r| r.chunks_exact(4).map(|w| i32::from_le_bytes(w.try_into().unwrap())).collect())
                .unwrap_or_default()
        };
        let v = words(b'v');
        let lines = v.chunks_exact(2).take_while(|p| p[0] >= 0).map(|p| (p[0] as u32, (p[1] / 5).max(0) as u32)).collect();
        let y = words(b'y');
        let replies = y.iter().take_while(|&&w| w >= 0).map(|&w| w as u32).collect();
        let lost = y.iter().position(|&w| w < 0).filter(|&i| y[i] == -2).and_then(|i| y.get(i + 1)).filter(|&&w| w >= 0).map(|&w| w as u32);
        let mut areas = vec![Vec::new()];
        for &w in words(b'x').iter().take_while(|&&w| w != -1) {
            match w {
                -2 => areas.push(Vec::new()),
                w => areas.last_mut().unwrap().push(w as u32),
            }
        }
        Self { lines, replies, lost, areas }
    }

    /// The clip and length (frames) of `line` in `g`'s voice. The 0xe0
    /// sets (body flag 0x4000) are not used: what sets that flag is not
    /// traced.
    fn line(&self, line: u32, g: &Guard) -> Option<(u32, u32)> {
        let i = (line & 0xFFF) + g.voice() * LINES_PER_VOICE;
        let (id, frames) = *self.lines.get(i as usize)?;
        Some((id, if line & 0x0800_0000 != 0 { frames } else { 0xF0 }))
    }
}

impl Phase {
    /// Starts `line` (`FUN_0021cdb8`): queues its clip and the click.
    /// Returns the clip.
    fn speak(&mut self, line: u32, g: &Guard) -> Option<u32> {
        let (id, frames) = self.radio.line(line, g)?;
        self.play_line(line, id);
        self.on_air = Some((line >> 28, frames));
        Some(id)
    }

    /// `FUN_0021d230`: the priority of the line on the air, -1 when none.
    fn air_priority(&self) -> i32 {
        self.on_air.map_or(-1, |(p, _)| p as i32)
    }

    fn play_line(&mut self, line: u32, id: u32) {
        if line & 0x4_0000 == 0 {
            crate::sfx::play(RADIO_CLICK, None);
        }
        self.say.push(id);
    }

    /// A soldier's radio line cut short (`FUN_0021d0a8`): the clip stops
    /// and, outside an alert, the manager opens a "no answer" slot for him
    /// (`FUN_0021d860(.., 900, 1)`; one per soldier, eight at most).
    fn cut(&mut self, id: u32, route: usize) {
        self.stop = Some(id);
        if self.kind != PhaseKind::Alert && self.no_answer.len() < 8 && !self.no_answer.iter().any(|s| s.0 == route) {
            info!("guard {route}: radio cut off, HQ waits for an answer");
            self.no_answer.push((route, NO_ANSWER_FRAMES));
        }
    }

    /// One frame of the slots (`FUN_002211d0`, kind 1): at 600 frames
    /// left HQ asks (0x1081400a, line 0x1ca of `-v`: "What's going on?
    /// Respond!"), at 300 it reports the lost contact (0x914000 /
    /// 0x914100: `-y`'s -2 clip); the slot then runs out.
    fn no_answer_tick(&mut self) {
        let mut calls = Vec::new();
        for s in &mut self.no_answer {
            match s.1 {
                600 => calls.push(HQ_RESPOND),
                300 => calls.push(HQ_LOST),
                _ => {}
            }
            s.1 -= 1;
        }
        self.no_answer.retain(|s| s.1 > 0);
        for c in calls {
            let id = if c == HQ_LOST { self.radio.lost } else { self.radio.lines.get((c & 0xFFF) as usize + 0x1C0).map(|l| l.0) };
            if let Some(id) = id {
                self.play_line(c, id);
            }
        }
    }

    /// The length (frames) of `line` for `g`, `fallback` when it cannot
    /// play (the caller's value for `FUN_0021cdb8` < 0).
    fn line_frames(&self, line: u32, g: &Guard, fallback: u32) -> u32 {
        self.radio.line(line, g).map_or(fallback, |l| l.1)
    }

    /// HQ's reply to a report (`0x2000000 | n`, 0xf0 frames; the click
    /// plays first).
    fn reply(&mut self) {
        if let Some(&id) = self.radio.replies.first() {
            crate::sfx::play(RADIO_CLICK, None);
            self.say.push(id);
        }
    }

    /// HQ's answer to a call for backup (`FUN_0021cb40` queues
    /// `FUN_0021c940` after 0x2c000000 / 2 / 4 and 0x20008003): one of the
    /// `-x` clips of the first area whose trap holds Snake
    /// (`FUN_0021c670`; `FUN_00148a98` at 0x1ece00, taken as Snake's
    /// position), picked at random (`pick`), else line 0x1c4. Played at
    /// once, with the click.
    fn area_line(&mut self, snake: Vec3, game: &Game, pick: u32) {
        let p = snake + Vec3::Y * 10.0;
        let area = game.data.collision.as_ref().and_then(|h| {
            AREA_TRAPS.iter().enumerate().find_map(|(k, &(name, entry))| {
                let listed = self.radio.areas.get(k).is_some_and(|a| !a.is_empty());
                let inside = h.traps.iter().any(|t| t.name & 0x00FF_FFFF == name && (0..3).all(|i| t.min[i] <= p[i] && p[i] < t.max[i]));
                (listed && inside).then_some(entry)
            })
        });
        let clip = match area.and_then(|a| self.radio.areas.get(a)).filter(|a| !a.is_empty()) {
            Some(clips) => Some(clips[pick as usize % clips.len()]),
            None => self.radio.lines.get((HQ_CHECK_IT_OUT & 0xFFF) as usize + 0x1C0).map(|l| l.0),
        };
        info!("HQ answers the call (area {area:?})");
        if let Some(id) = clip {
            self.play_line(HQ_CHECK_IT_OUT, id);
        }
    }
}

impl Default for Phase {
    fn default() -> Self {
        Self {
            kind: PhaseKind::Infiltration,
            meter: 0,
            evasion: 0,
            caution: 0,
            caution_time: 0xE10,
            last_seen: None,
            changed: false,
            blast: 0,
            radio: Radio::default(),
            say: Vec::new(),
            stop: None,
            no_answer: Vec::new(),
            on_air: None,
            callers: Vec::new(),
            report_at: Vec3::ZERO,
            jammed: false,
            atc: AtcommRun { left: 16, ..Default::default() },
            unseen: 0,
            route_events: Vec::new(),
            route_lines: Vec::new(),
        }
    }
}

impl Phase {
    pub fn from_manager(s: Option<&Spawn>, script: &mgs2_formats::gcx::Gcx) -> Self {
        let mut p = Self::default();
        if let Some(s) = s {
            p.caution_time = s.opt_int(b'h', 0).unwrap_or(0xE10);
            p.atc.left = s.opt_int(b'b', 0).unwrap_or(16);
            p.radio = Radio::from_script(s, script);
            let c = |i| s.opt_int(b'c', i).map(|v| v as f32);
            if let (Some(x), Some(y), Some(z)) = (c(0), c(1), c(2)) {
                p.report_at = Vec3::new(x, y, z);
            }
            let vals = |letter| s.opt(letter).map(|v| v.to_vec()).unwrap_or_default();
            p.route_events = vals(b'e')
                .chunks_exact(3)
                .filter_map(|c| match (c[0].int(), c[1].int(), &c[2]) {
                    (Some(r), Some(n), crate::gcl::Val::Proc(proc)) => Some((r.max(0) as usize, n.max(0) as usize, *proc)),
                    _ => None,
                })
                .collect();
            p.route_lines = vals(b'f')
                .chunks_exact(4)
                .filter_map(|c| Some((c[0].int()?.max(0) as usize, c[1].int()?.max(0) as usize, c[2].int()? as u32, (c[3].int()? / 5).max(0) as u32)))
                .collect();
        }
        p
    }

    /// The `-e` proc for a soldier reaching `point` of `route`.
    fn route_event(&self, route: usize, point: usize) -> Option<u32> {
        self.route_events.iter().find(|e| e.0 == route && e.1 == point).map(|e| e.2)
    }

    /// The `-f` line for `point` of `route`: (clip, frames).
    fn route_line(&self, route: usize, point: usize) -> Option<(u32, u32)> {
        self.route_lines.iter().find(|l| l.0 == route && l.1 == point).map(|l| (l.2, l.3))
    }

    fn set(&mut self, k: PhaseKind) {
        if self.kind != k {
            self.kind = k;
            self.changed = true;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Vision {
    /// Near range `d` (controller +0x248).
    pub distance: f32,
    /// Half angle in 4096-units (controller +0x244).
    pub half_angle: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Normal 0: walking the route.
    Patrol,
    /// Waiting at a route point (frames left; infinite = -1 wait).
    Wait(f32),
    /// Normal 6: go and look (target, sub-state, frame counter).
    /// `asks`: normal state 6 (something seen), which says "Is there
    /// somebody there?"; noises lead to state 3 instead (`FUN_002345b0`),
    /// silent on its first pass (see `re/notes/guard-ai.md`).
    Investigate { target: Vec3, sub: u8, timer: u32, asks: bool },
    /// Normal 8: discovery ("!"), frame counter.
    Discovery { timer: u32 },
    /// Alert brain: chase.
    Chase,
    /// Evasion brain: search near the last known position (frames to wait).
    Search { wait: u32, facing: f32 },
    /// Going back to the route after alert/evasion/investigation.
    Return,
    /// Darted to sleep or knocked out: falls (motion 113) and lies
    /// (motion 114) until `timer` reaches `wake`.
    Asleep { timer: f32, wake: f32 },
    /// Shot dead with the USP: falls (motion 113) and stays down.
    Dead { timer: f32 },
    /// Hit by a round and still alive: knocked down (motion 0x7b) and
    /// getting up again (frames so far).
    Hurt { timer: f32 },
    /// Normal 2 (stimulus 0x40, `FUN_00233120`): a comrade down in sight;
    /// `sub` is the game's sub-state (+0x330), `timer` its frames (+0x338).
    /// See [`comrade`].
    Comrade { target: Entity, at: Vec3, sub: u8, timer: u32 },
    /// Held up at gunpoint: hands raised (motion 8). `loose` counts frames
    /// without a gun on him; `shake` counts frames with Snake aiming from
    /// the front (dog tags at [`TAG_FRAMES`]).
    HeldUp { loose: f32, shake: f32 },
    /// Let go of a hold-up: he calls it in (see [`holdup_call`]). `to`:
    /// where he turns (his target heading +0xf54, last set toward Snake).
    HoldupCall { sub: u8, timer: u32, to: Vec3 },
    /// Normal 0xf (stimulus 0x8000000, `FUN_00245db0` / `FUN_00245d18`):
    /// his check-in at a route point with the radio shot or jammed (see
    /// [`check_in`]).
    CheckIn { sub: u8, timer: u32 },
    /// After the discovery "!": the call for backup (see [`call`]); `sub`
    /// is the discovery table's sub-state (+0x330), `timer` its frames,
    /// `line` the call it will make (+0x310), `to` where he backs off to.
    Call { sub: u8, timer: u32, line: u32, to: Option<Vec3> },
    /// Normal 11: walked into Snake's box and looks it over
    /// (`FUN_00242448`), frames so far.
    BoxCheck { timer: u32 },
    /// Normal 7 (`FUN_0023cb78`, stimulus 0x200000: a dart that left him
    /// awake): he turns toward where Snake was and looks about. See
    /// [`stung`].
    Stung { at: Vec3, sub: u8, timer: u32 },
}

/// The motions a guard plays (indices into his motion archive).
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    pub idle: usize,
    pub walk: usize,
    /// How fast the walk carries him (mm/s): its root travel over its
    /// length at 60 frames a second.
    pub walk_speed: f32,
    pub run: usize,
    /// Rifle raised.
    pub aim: usize,
    pub hands_up: usize,
    /// Falling asleep, then lying.
    pub fall: usize,
    pub lie: usize,
    /// The discovery "!" reaction.
    pub exclaim: usize,
    /// Looking around at the end of an investigation (`None`: idle and
    /// turning).
    pub look: Option<usize>,
    /// Knocked down by a hit and getting up (`None`: no reaction).
    pub knocked: Option<(usize, usize)>,
    /// Getting up after sleeping (motion, frames): when the sleep runs
    /// out, and when a comrade wakes him (`None`: straight up).
    pub get_up: Option<((usize, f32), (usize, f32))>,
    /// Seeing to a comrade who is down ([`comrade`]); `None`: he only goes
    /// over and looks.
    pub check: Option<Check>,
}

/// The motions of seeing to a downed comrade and calling it in (the
/// action table `FUN_00231948` and the radio's motion functions).
#[derive(Clone, Copy, Debug)]
pub struct Check {
    /// Action 8 (`FUN_00231828`, 0x4c): kneels and shakes a knocked-out
    /// comrade, the shake landing on frame 22.
    pub shake: usize,
    /// Action 9 (`FUN_0025eaf0`, 0x2f): kicks a sleeper awake.
    pub kick: usize,
    /// Action 4 (`FUN_0025ebc0`): takes the radio out (0x23; the radio is
    /// in his left hand from frame 28), then `FUN_0025eda0` talks (0x1b,
    /// looped, at most 600 frames) and action 5 / `FUN_0025f068` puts it
    /// away (0x1c; back on the belt at frame 62). The wounded-arm variants
    /// (0x31 / 0x32 / 0x33, radio in the right hand) are not used.
    pub radio_out: usize,
    pub radio_talk: usize,
    pub radio_away: usize,
    /// The radio dead (action 6 of state 2 / 7 of the discovery,
    /// `FUN_0025ef08`): motion 10 (120 frames; 0x30 with the left arm
    /// hurt), then `FUN_0025f068` puts it away.
    pub radio_fails: usize,
}

impl Anim {
    /// Gurlukovich soldiers, `gbs.mar`.
    pub const SOLDIER: Anim = Anim { idle: motion::IDLE, walk: motion::WALK, walk_speed: WALK_SPEED, run: motion::RUN, aim: motion::AIM, hands_up: 8, fall: sleep::FALL, lie: sleep::LIE, exclaim: motion::EXCLAIM, look: Some(motion::LOOK), knocked: Some((hurt::FALL, hurt::GET_UP)), get_up: Some(((sleep::GET_UP, 95.0), (sleep::ROUSED, 175.0))), check: Some(Check { shake: 76, kick: 47, radio_out: 35, radio_talk: 27, radio_away: 28, radio_fails: 10 }) };
    /// The Marines who walk the holds' routes (crowd members placed at the
    /// origin, w04c `FUN_002302a8`): the soldiers' brain over `ushold.mar`,
    /// whose motions from 38 on are the soldiers' set by slot (`FUN_0022fbe8`
    /// fills slot k with motion k + 0x26, then the pairs at 0x1f37a0: idle
    /// 0x1f, walk 0x20, slot 5 0x21). About half are the Marines' own
    /// versions; the aim, the look about and the radio are a one-frame
    /// placeholder (0x5fc4), so they aim from the idle pose and do not
    /// look about. 32 walks 1133 mm in 59 frames; 40 runs as the soldiers
    /// do; 46 hands up, 151 / 152 fall asleep and lie, 130 the "!", 161 /
    /// 168 knocked down and up, 170 / 171 get up (95 / 175 frames).
    pub const MARINE: Anim = Anim {
        idle: 31,
        walk: 32,
        walk_speed: 1132.7 / 59.0 * 60.0,
        run: 40,
        aim: 31,
        hands_up: 46,
        fall: 151,
        lie: 152,
        exclaim: 130,
        look: None,
        knocked: Some((161, 168)),
        get_up: Some(((170, 95.0), (171, 175.0))),
        check: None,
    };
    /// The commandant (variant 19, `-a` flag 0x20 on his walker): slots 0,
    /// 1, 3 and 0x3d become 0x23, 0x22, 0x25 and 0x24 (`FUN_0022fbe8`). His
    /// walk (34) covers 912 mm in 157 frames: he paces.
    pub const COMMANDANT: Anim = Anim { idle: 35, walk: 34, walk_speed: 912.2 / 157.0 * 60.0, ..Anim::MARINE };
}
/// A held-up guard calls it in after this long without a gun on him
/// (`FUN_0023af20`: 0x79 frames).
const HOLDUP_LOOSE: f32 = 121.0;
/// Snake's facing within this of the guard covers him (`FUN_002541c8`:
/// the angle between the guard's bearing to Snake and Snake's facing over
/// 0x5ff of 0x800).
const HOLDUP_ANGLE: f32 = 0x200 as f32 * ANGLE;
/// The hold-up's report (`FUN_0023a1e8(.., 0x1c000007, 0x200)`).
pub const LINE_HOLDUP: u32 = 0x1C00_0007;
/// A soldier's drop list: `-q` / `-a` (proc, chance) pairs.
fn drop_list(s: &Spawn, letter: u8) -> Vec<(u32, i32)> {
    s.opt(letter)
        .unwrap_or(&[])
        .chunks_exact(2)
        .take(4)
        .filter_map(|c| match (&c[0], c[1].int()) {
            (crate::gcl::Val::Proc(p), Some(chance)) => Some((*p, chance)),
            _ => None,
        })
        .collect()
}

/// Drop procs to run: (proc, arguments) (`FUN_00245e98`: map, x y z, flag,
/// 0, 0, the soldier's name).
pub static DROPS: std::sync::Mutex<Vec<(u32, Vec<i32>)>> = std::sync::Mutex::new(Vec::new());

/// Tries the soldier's next drop from list `which` (0 `-q`, 1 `-a`) at
/// `scale` times its chance (`FUN_00245e98`).
fn try_drop(g: &mut Guard, which: usize, scale: f32, at: Vec3, rng: &mut Rng) {
    let Some(&(proc, chance)) = g.drops[which].get(g.dropped[which]) else { return };
    if (rng.next() % 100) as f32 >= chance as f32 * scale {
        return;
    }
    g.dropped[which] += 1;
    if let Ok(mut q) = DROPS.lock() {
        q.push((proc, vec![0, at.x as i32, at.y as i32, at.z as i32, 0, 0, 0, g.name]));
    }
}

/// Marks a soldier's dog tags (with his route) in `Inventory::taken`.
const DOG_TAG_KEY: u32 = 0x8000_0000;
/// Aiming at a held-up guard from the front this long gets his dog tags.
const TAG_FRAMES: f32 = 120.0;
/// Hold-ups work within this range (mm).
const HOLDUP_RANGE: f32 = 2500.0;

/// gbs.mar: knocked down by a hit (0x7b, the motion the damage code picks
/// while life remains) and getting up from lying (130, assumed from its
/// poses).
mod hurt {
    pub const FALL: usize = 123;
    pub const FALL_FRAMES: f32 = 119.0;
    pub const GET_UP: usize = 130;
    pub const GET_UP_FRAMES: f32 = 119.0;
}

mod sleep {
    /// gbs.mar: falling prone, lying asleep.
    pub const FALL: usize = 113;
    pub const LIE: usize = 114;
    /// Getting up from lying on his back (0x84) when the sleep runs out,
    /// or (0x85) when shaken awake (`FUN_0024e870`).
    pub const GET_UP: usize = 132;
    pub const ROUSED: usize = 133;
    pub const FALL_FRAMES: f32 = 140.0;
}
/// Evasion: frames spent looking around at each stop.
const LOOK_FRAMES: u32 = 180;
/// Life without `-l`.
const DEFAULT_LIFE: i32 = 3000;
/// Stamina without `-f`.
const DEFAULT_STAMINA: i32 = 9;
/// Dart resistance (`FUN_0025ad08` passes 6000; `FUN_00247d20` halves it).
const RESIST: i32 = 3000;
/// Hearing range for noises: the soldier's `-h` (`FUN_00222f60` keeps it
/// at controller +0x24c, which `FUN_0021f958` compares), default 12000 mm.
/// w00a passes 4000, later stages As16:0x1c2.
const HEARING: f32 = 12000.0;

/// A soldier's line (`id` + his voice), through the game's queue
/// (`FUN_0021c620`): it plays at once, and no other queued line plays in
/// the next 30 frames.
pub fn say(id: u32, g: &Guard, at: Vec3) {
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.is_some_and(|t| t.elapsed().as_secs_f32() < 0.5) {
        return;
    }
    *last = Some(std::time::Instant::now());
    crate::sfx::play(id + g.voice(), Some(at));
}

/// The holds' Marines are not soldiers (their crowd actor has its own
/// "heard something" event, not ported); their range is assumed.
const MARINE_HEARING: f32 = 3000.0;

/// The loudest noise this frame (level, place), as `FUN_0011b630` keeps it
/// for the soldiers (`FUN_00220078` reads it next frame).
static NOISE: std::sync::Mutex<Option<(u8, Vec3)>> = std::sync::Mutex::new(None);

/// The noise the soldiers heard this frame, for other listeners (the
/// holds' crowd, `FUN_002286f0`).
pub static HEARD: std::sync::Mutex<Option<(u8, Vec3)>> = std::sync::Mutex::new(None);

/// A noise of `level` at `at` (mm); the loudest of a frame is heard.
pub fn noise(level: u8, at: Vec3) {
    if level == 0 {
        return;
    }
    if let Ok(mut n) = NOISE.lock()
        && n.is_none_or(|(l, _)| level > l)
    {
        *n = Some((level, at));
    }
}
/// How long a tranquilized guard sleeps: the enemy manager's `-z`
/// (`FUN_00221970`, default 18000 frames = 5 minutes; every Tanker stage
/// passes 18000), counted down at body +0xf1a.
const SLEEP_FRAMES: f32 = 18000.0;
/// Knocked out (stamina +0xf0e gone): the manager's `-o` (default and
/// every stage 1800 frames) plus 0..63 at random (`FUN_0024da30`).
const KNOCKOUT_FRAMES: f32 = 1800.0;

#[derive(Component)]
pub struct Guard {
    pub route: usize,
    pub node: usize,
    pub state: State,
    pub vision: Vision,
    /// Perception result (controller +0x18E).
    pub level: u8,
    /// Suspicion meter (controller +0x274).
    pub meter: i16,
    /// Tracking flag (+0x348 & 0x40) and its timer (+0x33C).
    tracking: Option<u32>,
    /// Last position Snake was seen at (+0x200).
    pub last_seen: Option<Vec3>,
    path: Vec<Vec3>,
    repath: u32,
    attack: Attack,
    /// Distance (mm) at which the guard hears a noise (`-h`).
    pub hearing: f32,
    /// Life (body +0xf0c): `-l`, As16:0x1c4 by difficulty (3000 / 3000 /
    /// 3000 / 4000 / 6000), default 3000 (`FUN_0025ad08` via
    /// `FUN_00247d20`).
    pub life: i32,
    max_life: i32,
    /// Wounded limbs (+0xf16 bits 0..3: the arms, then the legs).
    pub wounds: u8,
    /// Stamina against blows (+0xf0e): `-f`, default 9; refilled when he
    /// comes round.
    pub stamina: i32,
    max_stamina: i32,
    /// Resistance to the darts (+0xf12, from +0xf0a: 3000); asleep when it
    /// drops below 0.
    pub resist: i32,
    /// Darts in him by zone (+0xf21 head, +0xf22 limbs, +0xf23 body).
    darts: [u8; 3],
    /// Woken by a comrade (gets up with the other motion).
    roused: bool,
    /// Down from a blow rather than the darts (+0xffa 2, not 1).
    knocked: bool,
    /// A body already seen to (+0x34c bit 4, `FUN_00232e60`).
    checked: bool,
    /// The radio in his left hand (radio flag 0x2) during a call.
    pub radio_in_hand: bool,
    /// His radio found dead in this call (controller +0x348 bit 4, set on
    /// frame 0x73 of the dead-radio action): he reports in person.
    radio_dead: bool,
    /// He has called this discovery in (controller +0x344 bit 8: set as
    /// sub-states 3 and 0x15 go to the radio, 0xe).
    called: bool,
    /// His radio shot (`radio.c` +0xb4 bit 0x10000, `FUN_00265f50`): his
    /// calls fail as under chaff until he reports in person, which clears
    /// it (`FUN_00232050` / `FUN_0023a610` / `FUN_00237d48`).
    pub radio_shot: bool,
    /// Out of play by the script (message 0x106, +0x350 bit 1): hidden,
    /// no brain, no senses, not hit; message 0x107 brings him back.
    pub inactive: bool,
    /// A route the script gave him (message 1): (route, start point),
    /// taken up at his next route step.
    pending_route: Option<(usize, Option<usize>)>,
    /// The script's message 2 arrived (+0x34c bit 8, good for one frame:
    /// `signal` until his turn this frame, then `signal_now`).
    signal: bool,
    signal_now: bool,
    /// Frames he has spent at his current route point.
    point_frames: u32,
    /// The gait for the leg to the next route point ([`pick_gait`]).
    gait: u8,
    /// His eyes off Snake this frame: body flag 0x1000 from the action his
    /// route point plays ([`point_blind`]), which the sight
    /// (`FUN_002559c0`) reads as nothing seen.
    blind: bool,
    /// The body's turn from the way he walks while side-stepping (gaits 2
    /// / 3), taken off again before he moves on.
    side_turn: f32,
    /// The radio line he is saying (its clip), until it ends.
    on_air: Option<u32>,
    /// Seconds spent walking without getting anywhere.
    stuck: f32,
    /// Where he goes in an alert once Snake is out of sight (`-d`).
    pub post: Option<Vec3>,
    /// Has given up his dog tags.
    pub tagged: bool,
    /// What he drops (`-q` when he goes down, `-a` when shaken down after
    /// his tags; `FUN_00245e08`): up to four (proc, chance %), used in
    /// order, and how many of each list have dropped.
    pub drops: [Vec<(u32, i32)>; 2],
    pub dropped: [usize; 2],
    /// His actor name (the drop procs pick the item by it).
    pub name: i32,
    /// A `defender.c` soldier (see [`DEFENDER_ACTOR`]).
    pub defender: bool,
    /// Walks through the level's walls (the commandant: w04c's route 0
    /// starts inside the wedge of walls round the stage's middle, x
    /// -460..460, z -10220..-12180, which the game's walker must not be
    /// stopped by; how its movement treats walls (`hzxd.c`) is not traced,
    /// one-sided walls being the likely reason. Assumed.)
    pub ghost: bool,
    pub defend: Defend,
    /// His head turned from his facing (4096ths): now, and where a state
    /// points it this frame (body +0xf66 under controller +0x354 bit 0x800).
    /// See [`Guard::turn_head`].
    head: f32,
    head_to: f32,
    /// A dart hit him this frame and left him awake (body +0xf98 bit
    /// 0x800000000, `FUN_0024f490`; stimulus 0x200000 via `FUN_00230c20`).
    stung: bool,
    pub anim: Anim,
}

/// Where Snake fired an unsuppressed shot (the USP: noise level 10) for the
/// guards to hear.
#[derive(Resource, Default)]
pub struct Gunshot(pub Option<Vec3>);

/// Shooting (`FUN_00228710`/`FUN_00228F80`): aim for 21 frames, then a
/// burst with one shot every 4 frames and a 1-in-3 chance to stop after
/// each; reload after 31 shots. Counters are in frames (60 per second).
#[derive(Clone, Copy, Debug, Default)]
struct Attack {
    aim: f32,
    cooldown: f32,
    firing: bool,
    shots: u32,
    reload: f32,
    /// Frames into a rifle-butt strike, and whether it has landed.
    strike: Option<(f32, bool)>,
}

/// Close quarters (`FUN_00228710` sub-state 6, `FUN_00229b90`): Snake
/// seen within this (mm) gets the rifle butt instead of a burst: alert
/// action 5 (`FUN_002281e8` -> `FUN_0025f810`), motion 0x55, whose hit box
/// rides joint 20 on frames 2-5 (`FUN_00254248`).
const STRIKE_RANGE: f32 = 1000.0;
const STRIKE_MOTION: usize = 0x55;
/// What a strike takes from Snake: `FUN_00254248`'s hit record
/// (`FUN_001081d0(.., 0, 0xff, 0, 5, ..)`, damage at +0x14 as the rifle
/// round's 10) and its box: half-extents 500 mm (w00a 0x1f3c40) on the
/// joint's place, tested box against box (`FUN_00108e38`).
const STRIKE_DAMAGE: i32 = 5;
const STRIKE_BOX: f32 = 500.0;
const STRIKE_JOINT: usize = 20;
/// With a wounded arm (+0xf16 & 1) the alert strikes with action 9
/// (`FUN_0025fa48`): motion 0x54 (0x153 under body flag 0x4000, not
/// traced), whose box rides joint 16 (`FUN_00254248`).
const WOUNDED_STRIKE_MOTION: usize = 0x54;
const WOUNDED_STRIKE_JOINT: usize = 16;
/// Frames of the motion that carry the hit box (`FUN_0025f810`: 2-5).
const STRIKE_FRAMES: std::ops::RangeInclusive<f32> = 2.0..=5.0;

/// Whether the strike's box on joint 20 meets Snake's (feet at `snake`;
/// his box is assumed to be his collision cylinder's: [`RADIUS`] across,
/// [`HEIGHT`] tall).
fn strike_hits(game: &Game, body: &Body, joint: usize, snake: Vec3) -> bool {
    let model = &game.data.models[body.model].1;
    let motion = body.motions.and_then(|m| game.data.motions[m].1.motions.get(body.motion));
    let Some(joint) = mgs2_bevy::anim::pose(model, motion, body.frame, false).get(joint).map(|m| Vec3::from(m.translation)) else {
        return false;
    };
    let at = body.pos + Quat::from_rotation_y(body.yaw) * joint;
    let d = (at - (snake + Vec3::Y * HEIGHT / 2.0)).abs();
    d.x <= STRIKE_BOX + RADIUS && d.z <= STRIKE_BOX + RADIUS && d.y <= STRIKE_BOX + HEIGHT / 2.0
}

/// Strikes that landed this frame (damage, push in mm per frame), taken by
/// the player. The push is (0, -100, 100) turned by the soldier's facing
/// (w00a 0x1f3c30): 100 mm a frame away from him (the drop is not used).
pub static STRIKES: std::sync::Mutex<Vec<(i32, Vec3)>> = std::sync::Mutex::new(Vec::new());
const STRIKE_PUSH: f32 = 100.0;

fn land_strike(body: &Body) {
    if let Ok(mut s) = STRIKES.lock() {
        s.push((STRIKE_DAMAGE, body.forward() * STRIKE_PUSH));
    }
}

const AIM_FRAMES: f32 = 21.0;
const SHOT_FRAMES: f32 = 4.0;
const MAGAZINE: u32 = 31;
/// Reload (`FUN_00228f80` after 31 shots -> sub-state 5: alert action 7,
/// `FUN_002281e8` -> motion 0x3e, 0x155 under flag 0x4000) lasts the
/// motion; this if the motion is missing.
const RELOAD_FRAMES: f32 = 90.0;
const RELOAD_MOTION: usize = 0x3E;
/// Guards aim when they see Snake (level 3, inside their near range) or
/// when he is closer than this (`FUN_00228710`: 0x5dd mm).
const POINT_BLANK: f32 = 1501.0;
/// Muzzle height above the feet (approximate).
const MUZZLE: f32 = 1350.0;

/// Builds a guard from its script spawn.
pub fn from_spawn(s: &Spawn, default_vision: f32) -> Option<(Guard, Vec3)> {
    let route = s.opt_int(b'r', 0)? as usize;
    let node = s.opt_int(b'n', 0).unwrap_or(0).max(0) as usize;
    // `-d x y z map`: where he goes in an alert (controller +0x2b0); also
    // the fallback start when his route is missing.
    let post = s.opt(b'd').is_some().then(|| {
        Vec3::new(s.opt_int(b'd', 0).unwrap_or(0) as f32, s.opt_int(b'd', 1).unwrap_or(0) as f32, s.opt_int(b'd', 2).unwrap_or(0) as f32)
    });
    let pos = post.unwrap_or(Vec3::ZERO);
    let post_at = post;
    // `-i` d0 [d1 d2 d3]; FUN_00222F60 defaults 6000; vision uses d0.
    let distance = s.opt_int(b'i', 0).filter(|d| *d > 0).map_or(default_vision, |d| d as f32);
    let life = s.opt_int(b'l', 0).filter(|d| *d > 0).unwrap_or(DEFAULT_LIFE);
    let stamina = s.opt_int(b'f', 0).filter(|d| *d > 0).unwrap_or(DEFAULT_STAMINA);
    let hearing = s.opt_int(b'h', 0).filter(|d| *d > 0).map_or(HEARING, |d| d as f32);
    let guard = Guard {
        route,
        node,
        state: State::Patrol,
        vision: Vision { distance, half_angle: 0x200 },
        level: 0,
        meter: 0,
        tracking: None,
        last_seen: None,
        path: Vec::new(),
        repath: 0,
        attack: Attack::default(),
        hearing,
        life,
        max_life: life,
        // MGS2_WOUND=route:bits: start that soldier wounded (testing).
        wounds: std::env::var("MGS2_WOUND")
            .ok()
            .and_then(|v| v.split(',').find_map(|e| e.split_once(':').filter(|(r, _)| r.parse() == Ok(route)).and_then(|(_, b)| b.parse().ok())))
            .unwrap_or(0),
        stamina,
        max_stamina: stamina,
        resist: RESIST,
        darts: [0; 3],
        roused: false,
        knocked: false,
        checked: false,
        radio_in_hand: false,
        radio_dead: false,
        called: false,
        radio_shot: false,
        inactive: false,
        pending_route: None,
        signal: false,
        signal_now: false,
        point_frames: 0,
        gait: 0,
        blind: false,
        side_turn: 0.0,
        on_air: None,
        stuck: 0.0,
        post: post_at,
        tagged: false,
        drops: [drop_list(s, b'q'), drop_list(s, b'a')],
        dropped: [0, 0],
        name: s.args.first().and_then(crate::gcl::Val::int).unwrap_or(0),
        defender: s.kind == DEFENDER_ACTOR,
        ghost: false,
        defend: Defend { fixed: s.opt_int(b't', 0).is_some_and(|t| t >= 2), ..Default::default() },
        head: 0.0,
        head_to: 0.0,
        stung: false,
        anim: Anim::SOLDIER,
    };
    Some((guard, pos))
}

/// A guard walking `route` from its start (the Marines who patrol the
/// holds).
pub fn patrol(route: usize, vision: f32, anim: Anim) -> Guard {
    Guard {
        route,
        node: 0,
        state: State::Patrol,
        vision: Vision { distance: vision, half_angle: 0x200 },
        level: 0,
        meter: 0,
        tracking: None,
        last_seen: None,
        path: Vec::new(),
        repath: 0,
        attack: Attack::default(),
        hearing: MARINE_HEARING,
        life: DEFAULT_LIFE,
        max_life: DEFAULT_LIFE,
        wounds: 0,
        stamina: DEFAULT_STAMINA,
        max_stamina: DEFAULT_STAMINA,
        resist: RESIST,
        darts: [0; 3],
        roused: false,
        knocked: false,
        checked: false,
        radio_in_hand: false,
        radio_dead: false,
        called: false,
        radio_shot: false,
        inactive: false,
        pending_route: None,
        signal: false,
        signal_now: false,
        point_frames: 0,
        gait: 0,
        blind: false,
        side_turn: 0.0,
        on_air: None,
        stuck: 0.0,
        post: None,
        tagged: false,
        drops: [Vec::new(), Vec::new()],
        dropped: [0, 0],
        name: 0,
        defender: false,
        ghost: false,
        defend: Defend::default(),
        head: 0.0,
        head_to: 0.0,
        stung: false,
        anim,
    }
}

/// The soldier's head (`FUN_00112448` puts the look's turn in joint slot
/// 12, flag 0x1000; the Marines' head is joint 12 too).
const HEAD_JOINT: usize = 12;
/// How far the head turns from the facing (w00a table 0x1f3c98: yaw
/// 0x400 either way; 0x1f3c90, all 0x7ff, under +0x354 bit 0x20000).
const HEAD_LIMIT: f32 = 1024.0;

impl Guard {
    /// The head look, once per frame (`FUN_00111f40` with the yaw a state
    /// set this frame, from the soldier's draw): the turn from his facing,
    /// limited to [`HEAD_LIMIT`], closes an eighth of the way each frame
    /// (`FUN_001419c8`). His sight follows the head (`FUN_002559C0` takes
    /// +0x240, the head matrix's yaw). Without a target he looks ahead
    /// (assumed: the game then looks at a point, `FUN_00111d80`, not
    /// traced). Only the head joint turns (the game also turns joints 2,
    /// 11, 4 and 8 by parts of it, mask 0x27: not ported).
    fn turn_head(&mut self, body: &mut Body, frames: f32) {
        let to = std::mem::take(&mut self.head_to).clamp(-HEAD_LIMIT, HEAD_LIMIT);
        let d = to - self.head;
        let k = 1.0 - (7.0f32 / 8.0).powf(frames.min(10.0));
        self.head = if d.abs() < 1.0 { to } else { self.head + d * k };
        body.turn = (self.head != 0.0).then(|| (HEAD_JOINT, Quat::from_rotation_y(self.head * ANGLE)));
    }

    /// Arriving in the middle of an alert: straight to the chase.
    pub fn join_alert(&mut self, last_seen: Option<Vec3>) {
        self.state = State::Chase;
        self.last_seen = last_seen;
        self.meter = METER_MAX;
    }

    /// Back where he started and nothing to do (a trooper may leave).
    pub fn idle_at_start(&self) -> bool {
        matches!(self.state, State::Patrol | State::Wait(_)) && self.node == 0
    }

    /// Which of the four voices he speaks with (the game takes a per-guard
    /// value mod 4 and adds it to each line's base id).
    pub fn voice(&self) -> u32 {
        (self.route % 4) as u32
    }

    /// Down: asleep, knocked out or dead.
    pub fn asleep(&self) -> bool {
        matches!(self.state, State::Asleep { .. } | State::Dead { .. })
    }

    pub fn dead(&self) -> bool {
        matches!(self.state, State::Dead { .. })
    }

    /// A lethal round (the USP).
    pub fn kill(&mut self) {
        if !self.dead() {
            self.state = State::Dead { timer: 0.0 };
            self.radio_in_hand = false;
            self.path.clear();
        }
    }

    /// A dart (`FUN_0024f490`): the head takes 3000 off his resistance,
    /// a limb 500, the body 1000, and each dart keeps draining it every
    /// frame ([`Guard::drowse`]).
    pub fn dart(&mut self, zone: Zone) {
        if self.asleep() {
            return;
        }
        let (cost, i) = match zone {
            Zone::Head => (3000, 0),
            Zone::Limb(_) => (500, 1),
            Zone::Body => (1000, 2),
        };
        self.resist -= cost;
        self.darts[i] = self.darts[i].saturating_add(1);
        if self.resist < 0 {
            self.fall_asleep();
        } else {
            self.stung = true;
        }
    }

    /// The darts' drain per frame (`FUN_00258528`), by difficulty
    /// (Bs16:0x8): Very Easy 3000 per dart; Easy 4 per limb dart and 8 per
    /// body dart; Normal and up 2 and 4. A head dart is always 3000.
    pub fn drowse(&mut self, ticks: u32, difficulty: i16) {
        if self.asleep() || self.darts == [0; 3] {
            return;
        }
        let [h, l, b] = self.darts.map(i32::from);
        let per_frame = match difficulty {
            d if d < 20 => (h + l + b) * 3000,
            d if d < 30 => h * 3000 + l * 4 + b * 8,
            _ => h * 3000 + l * 2 + b * 4,
        };
        self.resist -= per_frame * ticks as i32;
        if self.resist < 0 {
            self.fall_asleep();
        }
    }

    fn fall_asleep(&mut self) {
        self.state = State::Asleep { timer: 0.0, wake: SLEEP_FRAMES };
        self.knocked = false;
        self.radio_in_hand = false;
        self.path.clear();
    }

    /// Knocked out (punches, a stun grenade): down at once for
    /// [`KNOCKOUT_FRAMES`] and a little more.
    pub fn knock_out(&mut self) {
        // The random 0..63 frames, from his route and node.
        let extra = ((self.route * 37 + self.node * 11) % 64) as f32;
        self.state = State::Asleep { timer: 0.0, wake: KNOCKOUT_FRAMES + extra };
        self.knocked = true;
        self.radio_in_hand = false;
        self.path.clear();
    }

    /// A punch that does not knock him out: he turns on Snake.
    pub fn punched(&mut self, from: Vec3) {
        if !self.asleep() {
            self.last_seen = Some(from);
            if !matches!(self.state, State::Chase | State::Discovery { .. }) {
                self.state = State::Discovery { timer: 0 };
            }
        }
    }

    /// Back to the patrol with no memory of Snake (continuing after a game
    /// over).
    pub fn reset(&mut self) {
        self.state = State::Patrol;
        self.on_air = None;
        self.level = 0;
        self.meter = 0;
        self.tracking = None;
        self.last_seen = None;
        self.path.clear();
        self.attack = Attack::default();
        self.life = self.max_life;
        self.wounds = 0;
        self.stamina = self.max_stamina;
        self.resist = RESIST;
        self.darts = [0; 3];
    }

    /// A lethal round (`FUN_0024a1e8`): a head hit kills; the body takes
    /// 1000; a limb takes 500 and is wounded, and both arms or both legs
    /// wounded kill. Returns true when he dies.
    pub fn shot(&mut self, zone: Zone) -> bool {
        match zone {
            Zone::Head => self.life = 0,
            Zone::Body => self.life -= 1000,
            Zone::Limb(bit) => {
                self.life -= 500;
                self.wounds |= 1 << bit;
                let pair = if bit < 2 { 0b0011 } else { 0b1100 };
                if self.wounds & pair == pair {
                    self.life = 0;
                }
            }
        }
        if self.life <= 0 {
            self.kill();
            true
        } else {
            if !self.asleep() && self.anim.knocked.is_some() {
                self.state = State::Hurt { timer: 0.0 };
                self.path.clear();
            }
            false
        }
    }
}

/// Where a round strikes a guard.
#[derive(Clone, Copy, Debug)]
pub enum Zone {
    Head,
    Body,
    /// 0 / 1 the arms, 2 / 3 the legs (left, right).
    Limb(u8),
}

/// Signed difference of two 4096-unit angles (`FUN_001413B8`).
fn angle_diff(a: i32, b: i32) -> i32 {
    let d = (b - a) & 0xFFF;
    if d > 0x800 { d - 0x1000 } else { d }
}

fn to_units(rad: f32) -> i32 {
    ((rad / ANGLE).round() as i32) & 0xFFF
}

/// `FUN_002559C0` for the target Snake. Returns the perception level.
/// `height`: how tall the target stands (mm); the sight line goes to 60%.
/// `scale` shrinks both ranges (Snake in first person: three quarters).
fn perceive(g: &Guard, body: &Body, snake: Vec3, height: f32, game: &Game, scale: f32) -> u8 {
    let to = snake - body.pos;
    let dist = to.length();
    let (near, far) = (g.vision.distance * scale, g.vision.distance * 1.5 * scale);
    if far < dist {
        return 0;
    }
    let eye = body.pos + Vec3::Y * EYE;
    let target = snake + Vec3::Y * (height * 0.6);
    let blocked = || game.collision.as_ref().is_some_and(|c| c.line_blocked(eye, target));
    // FUN_00256C08(self, target, 250, 1500, 250): bumping into the guard.
    if to.x.abs() <= 250.0 && to.z.abs() <= 250.0 && to.y.abs() <= 1500.0 {
        return if blocked() { 1 } else { 3 };
    }
    // Horizontal: head direction vs direction to the target.
    let dir = to_units(to.x.atan2(to.z));
    let head = to_units(body.yaw + g.head * ANGLE);
    if angle_diff(head, dir).abs() > g.vision.half_angle {
        return 0;
    }
    // Vertical (FUN_0022DD78 minus 0x400, head pitch 0): within ±0x200.
    let rel = target - eye;
    let pitch = to_units(Vec2::new(rel.x, rel.z).length().atan2(rel.y)) - 0x400;
    if pitch.abs() > 0x200 {
        return 0;
    }
    if blocked() {
        1
    } else if dist <= near {
        3
    } else {
        2
    }
}

/// `FUN_00256488`.
fn update_meter(g: &mut Guard, phase: PhaseKind) {
    let m = g.meter;
    g.meter = match g.level {
        2 if phase == PhaseKind::Alert => m - 1,
        2 => m.max(1),
        3 => m + 1,
        _ if g.tracking.is_some() => m,
        _ => m - 16,
    }
    .clamp(0, METER_MAX);
}

/// Moves along `g.path` toward its end; returns true on arrival.
fn follow(g: &mut Guard, body: &mut Body, speed: f32, reach: f32, dt: f32, game: &Game) -> bool {
    while let Some(&next) = g.path.first() {
        let last = g.path.len() == 1;
        let flat = Vec2::new(next.x - body.pos.x, next.z - body.pos.z);
        if flat.length() <= if last { reach } else { 400.0 } {
            g.path.remove(0);
            continue;
        }
        body.yaw = turn_toward(body.yaw, flat.x.atan2(flat.y), TURN_RATE * dt);
        // A waypoint more than 45 degrees off is turned to on the spot (at
        // a run the turn, 645 mm across, is wider than the waypoint's
        // reach: he would circle it). Not the game's.
        let fwd = body.forward();
        if Vec2::new(fwd.x, fwd.z).dot(flat.normalize_or_zero()) < std::f32::consts::FRAC_1_SQRT_2 {
            return false;
        }
        let step = (speed * dt).min(flat.length());
        let mut p = body.pos + body.forward() * step;
        if let Some(col) = &game.collision {
            if !g.ghost {
                p = col.push_out(p, RADIUS, HEIGHT, STEP_UP);
            }
            match col.floor_below(p.x, p.z, p.y + STEP_UP) {
                Some(y) => p.y = y,
                None => {
                    g.path.remove(0);
                    continue;
                }
            }
        }
        // Barely moving (a wall, a corner): count it up for the brains.
        let moved = Vec2::new(p.x - body.pos.x, p.z - body.pos.z).length();
        g.stuck = if moved < step * 0.2 { g.stuck + dt } else { 0.0 };
        body.pos = p;
        return false;
    }
    g.stuck = 0.0;
    true
}

fn plan(g: &mut Guard, from: Vec3, to: Vec3, game: &Game) {
    g.path = match &game.nav {
        Some(nav) if !nav.is_empty() => nav.path(from, to),
        _ => vec![to],
    };
    g.repath = 30;
}

/// Where a route starts (troopers enter there).
pub fn route_start(game: &Game, route: usize) -> Option<Vec3> {
    waypoint(game, route, 0).map(|w| Vec3::from(w.position))
}

fn waypoint(game: &Game, route: usize, node: usize) -> Option<Waypoint> {
    let r = game.data.collision.as_ref()?.routes.get(route)?;
    r.points.get(node % r.points.len().max(1)).copied()
}

/// A route point's action index -> the walker's action (+0xc00), the table
/// at w00a 0x1f39f0 (`FUN_00224870`). 0x1d / 0x1e / 0x25 / 0x26 are gaits
/// for the walk to the point (`FUN_00224c28`: 2 / 3 / 7 / 9), not actions.
const POINT_ACTIONS: [u16; 26] = [0, 4, 5, 6, 0xA, 7, 8, 9, 0xD, 0xE, 0x10, 0x11, 0x12, 0x13, 0x15, 0x16, 0x17, 0x18, 2, 0x14, 0x1D, 0x1E, 0x25, 0x19, 0x1B, 0x26];

/// The walker's action at a point. With flags 0x40 / 0x80 the action index
/// is a motion number played through other steps (+0xc00 | 0x1000 /
/// 0x2000; not ported).
fn point_action(wp: &mgs2_formats::hzx::Waypoint) -> u16 {
    if wp.flags & 0xC0 != 0 {
        return 0;
    }
    POINT_ACTIONS.get(wp.action.max(0) as usize).copied().filter(|&a| a < 0x1D).unwrap_or(0)
}

/// The motion an action plays once while he waits (`FUN_002240f8`; the
/// step functions end it with +0xf58 = 1). Not ported: 3 / 0xf
/// (turns), 7 / 8 / 0xe / 0x10..0x14 / 0x17 (motions 274-282, one-frame
/// placeholders in gbs.mar), 0xa's effect at frame 18 and its motion 7,
/// 6's voice at frames 70, 226 and 393, 0x16 (idle, body flag
/// 0x100000000000) and 0x19 / 0x1a (the radio).
fn action_motion(action: u16) -> Option<usize> {
    Some(match action {
        // The "!" motion (`FUN_0025da40`, 0x5c): route action 18, and
        // flags 0x800 in Caution.
        2 => motion::EXCLAIM,
        4 => 15,
        5 => 16,
        6 => 17,
        9 => 231,
        0xA => 6,
        0xB => 20,
        0xC => 232,
        0x15 => 5,
        0x18 => 104,
        0x1B => 228,
        0x1C => 229,
        _ => return None,
    })
}

/// Action 0xd's head (`FUN_00223aa0`, a 270-frame cycle): front, then
/// +0x200 from frame 90 and -0x200 from frame 180 (the yaw +0xf66 under
/// +0x354 bit 0x800).
fn head_swing(t: u32) -> f32 {
    match t % 270 {
        90..180 => 512.0,
        180.. => -512.0,
        _ => 0.0,
    }
}

/// Whether the action at a point has ended (+0xf58 == 1), for waits of -1.
fn action_done(action: u16, t: u32, game: &Game, body: &Body) -> bool {
    match action {
        // `FUN_00223aa0` signals on frame 269 of its cycle.
        0xD => t >= 0x10D,
        a => action_motion(a).is_some_and(|m| t >= motion_length(game, body, m, 0)),
    }
}

/// `a` wrapped to -pi..pi.
fn signed_angle(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI
}

/// A route point's `-f` line: no click (0x40000), panned from him
/// (0x4000000), its length from the `-f` table (0x20000).
const ROUTE_LINE: u32 = 0x0406_0000;
/// The check-in's `-f` line (`FUN_00224530`): the same, with the click.
const CHECK_IN_LINE: u32 = 0x0402_0000;

/// Normal state 0xf (`FUN_00245d18`), a check-in with no radio:
/// - 1 (`FUN_00245d18` case 1): the radio out (action 2), then 0, or 2 if
///   it is still shot or jammed;
/// - 0 (`FUN_00245a00`): "The situation is normal." (0xc000012) on the
///   radio; cut short by a shot or jam (`FUN_0021d0a8`) -> 2; at the radio
///   away's end -> 6;
/// - 2 (`FUN_00245b70`): the dead radio (action 4; frame 0x73 he knows it),
///   then the run (gait 7) to the manager's `-c` point;
/// - 3 (`FUN_00245c08`): the run, until there (`FUN_00256910`);
/// - 4 (`FUN_00245c80`): there, the shot-radio flag and his limb wounds
///   cleared (`FUN_00255978`); 0x79 frames without a line, then 6 with
///   +0x348 cleared;
/// - 6: the state ends; he walks back to his route (assumed: to the point
///   he left, whose check-in then runs again).
fn check_in(g: &mut Guard, body: &mut Body, phase: &mut Phase, (sub, timer): (u8, u32), dt: f32, ticks: u32, game: &Game) {
    let Some(c) = g.anim.check else {
        g.state = State::Return;
        return;
    };
    let t = timer + ticks;
    let next = |sub| State::CheckIn { sub, timer: 0 };
    g.state = match sub {
        1 => {
            if !radio_out(g, body, c, t, game) {
                State::CheckIn { sub, timer: t }
            } else if no_radio(g, phase) {
                next(2)
            } else {
                next(0)
            }
        }
        0 => {
            let frames = phase.line_frames(LINE_ALL_CLEAR, g, REPORT_FRAMES);
            if t < frames && no_radio(g, phase) {
                cut_short(g, phase);
                next(2)
            } else {
                let (_, away) = radio_talk(g, body, phase, c, LINE_ALL_CLEAR, (timer, t), game);
                if away { next(6) } else { State::CheckIn { sub, timer: t } }
            }
        }
        2 => {
            if radio_fails(g, body, c, t, game) { next(3) } else { State::CheckIn { sub, timer: t } }
        }
        3 => {
            if run_to_report(g, body, phase, (timer, t), dt, game) { next(4) } else { State::CheckIn { sub, timer: t } }
        }
        4 => {
            body.play(g.anim.idle);
            if timer == 0 {
                info!("guard {} reports in person (check-in)", g.route);
                g.radio_shot = false;
                g.wounds = 0;
            }
            if t >= 0x79 {
                g.radio_dead = false;
                next(6)
            } else {
                State::CheckIn { sub, timer: t }
            }
        }
        _ => {
            g.radio_in_hand = false;
            if let Some(wp) = waypoint(game, g.route, g.node) {
                plan(g, body.pos, Vec3::from(wp.position), game);
            }
            State::Return
        }
    };
}

/// The point of `route` nearest to `at` (where `FUN_00257210` starts a
/// route given no point).
fn nearest_point(game: &Game, route: usize, at: Vec3) -> usize {
    game.data
        .collision
        .as_ref()
        .and_then(|c| c.routes.get(route))
        .and_then(|r| r.points.iter().enumerate().min_by(|a, b| Vec3::from(a.1.position).distance_squared(at).total_cmp(&Vec3::from(b.1.position).distance_squared(at))).map(|(i, _)| i))
        .unwrap_or(0)
}

/// The script's messages to the soldiers (`mesg <name> <code> args..`):
/// - 1 `route [x y z place [point]]` (`FUN_00222810`): a new route
///   (+0xc0f), taken up the next time he walks his route, and with the
///   rest a new post (+0x2b0, as `-d`) and start point (+0xc10, 0xff =
///   the nearest);
/// - 0x106 (`FUN_00257df8` -> `FUN_00257a68`): out of play: hidden (his
///   model and attachments +0x4c |= 0x3000) and his brain and senses
///   stopped (+0x350 bit 1); 0x107 back (+0x350 cleared, model shown);
/// - 2: +0x34c bit 8 for a frame, which ends a route point's wait of -3
///   (`FUN_002246c0`);
/// - 0x100 / 0x101 / 0x103 / 0x104 (+0x350 bits 0x80000000 / 0x100),
///   0x105 (sleep frames) and 3 / 4 (+0xd8 bit 2) are not ported (their
///   uses not traced).
pub fn messages(mut game: ResMut<Game>, mut phase: ResMut<Phase>, mut guards: Query<(&mut Guard, Option<&mut Visibility>)>) {
    if game.guard_messages.is_empty() {
        return;
    }
    // atcomm's message 1 n (`FUN_00272208`): light n is out (w03b's
    // searchlights when broken; the game clears its bit 12 frames later).
    let atcomm = mgs2_formats::cipher::str_code(b"atcomm") & 0x00FF_FFFF;
    for (_, _, args) in game.guard_messages.iter().filter(|m| m.0 == atcomm && m.1 == 1) {
        if let Some(&n) = args.first().filter(|n| (0..8).contains(*n)) {
            phase.atc.lights &= !(1u8 << n);
            info!("atcomm: light {n} out");
        }
    }
    if guards.is_empty() {
        // Not spawned yet (the stage's setup runs first): keep them a while.
        let n = game.guard_messages.len();
        game.guard_messages.drain(..n.saturating_sub(64));
        return;
    }
    for (target, code, args) in std::mem::take(&mut game.guard_messages) {
        for (mut g, vis) in guards.iter_mut().filter(|(g, _)| (g.name as u32) & 0x00FF_FFFF == target) {
            // A defender's route message is 3 `route [x y z map]`
            // (`FUN_00273828`; its 0 / 1 set +0x328 to 6 / 1, 2 moves him,
            // 4 / 5 its own flags: not ported).
            match code {
                3 if g.defender => {
                    let Some(&route) = args.first() else { continue };
                    if args.len() >= 4 {
                        g.post = Some(Vec3::new(args[1] as f32, args[2] as f32, args[3] as f32));
                    }
                    info!("guard {}: script gives route {route}", g.route);
                    g.pending_route = Some((route.max(0) as usize, None));
                }
                _ if g.defender => {}
                1 => {
                    let Some(&route) = args.first() else { continue };
                    let node = args.get(5).copied().filter(|n| (0..0xFF).contains(n)).map(|n| n as usize);
                    if args.len() >= 5 {
                        g.post = Some(Vec3::new(args[1] as f32, args[2] as f32, args[3] as f32));
                    }
                    info!("guard {}: script gives route {route}", g.route);
                    g.pending_route = Some((route.max(0) as usize, node));
                }
                2 => {
                    info!("guard {}: script signal", g.route);
                    g.signal = true;
                }
                0x106 | 0x107 => {
                    g.inactive = code == 0x106;
                    info!("guard {} {}", g.route, if g.inactive { "taken out of play" } else { "back in play" });
                    if let Some(mut v) = vis {
                        *v = if g.inactive { Visibility::Hidden } else { Visibility::Inherited };
                    }
                }
                _ => {}
            }
        }
    }
}

fn face(body: &mut Body, target: Vec3, dt: f32) {
    let to = target - body.pos;
    body.yaw = turn_toward(body.yaw, to.x.atan2(to.z), TURN_RATE * dt);
}

#[allow(clippy::too_many_arguments)]
pub fn think(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut phase: ResMut<Phase>,
    mut rng: ResMut<Rng>,
    mut inv: ResMut<crate::items::Inventory>,
    mut gunshot: ResMut<Gunshot>,
    first_person: Res<crate::fpv::FirstPerson>,
    jam: Option<Res<crate::grenade::Jam>>,
    snake: Query<(&Body, &Player), Without<Guard>>,
    mut guards: Query<(Entity, &mut Guard, &mut Body)>,
    mut clock: Local<f32>,
    mut down: Local<std::collections::HashSet<Entity>>,
    mut tested: Local<std::collections::HashSet<usize>>,
    mut forced: Local<bool>,
) {
    // The game's logic runs once per 60 Hz frame; counters advance by the
    // whole frames elapsed (movement uses the real time step).
    *clock += time.delta_secs().min(0.05) * 60.0;
    let ticks = clock.floor() as u32;
    *clock -= ticks as f32;
    // Chaff in the air jams the soldiers' radios (`DAT_0017f48c` bit 0x10).
    // MGS2_JAM=1 jams them for good (testing).
    phase.jammed = jam.is_some_and(|j| j.0 > 0.0) || std::env::var_os("MGS2_JAM").is_some();
    // Comrades lying on the floor (only after they have fallen), and how.
    // (The game's list behind `FUN_0021e998`, manager +0x7c, is filled
    // where the decomp does not show; taken here as the soldiers down.)
    let sleepers: Vec<(Entity, Vec3, Down)> = guards
        .iter()
        .filter_map(|(e, g, b)| match g.state {
            State::Asleep { timer, wake } if timer >= sleep::FALL_FRAMES && timer < wake => {
                Some((e, b.pos, if g.knocked { Down::KnockedOut } else { Down::Asleep }))
            }
            State::Dead { timer } if timer >= sleep::FALL_FRAMES && !g.checked => Some((e, b.pos, Down::Dead)),
            _ => None,
        })
        .collect();
    // Comrades someone is already seeing to (the game sends one soldier and
    // has a second who finds him there within 2 m stand back,
    // `FUN_00231ac0`; here the others leave him to it).
    let attended: Vec<Entity> = guards
        .iter()
        .filter_map(|(_, g, _)| match g.state {
            State::Comrade { target, .. } => Some(target),
            _ => None,
        })
        .collect();
    let mut woken = Vec::new();
    let mut seen_to = Vec::new();
    let dt = time.delta_secs().min(0.05);
    let Ok((snake_body, player)) = snake.single() else { return };
    let snake_pos = snake_body.pos;
    let snake_dead = player.dead();
    let snake_height = player.height();
    // The USP's light is on (as `usp_light`).
    let light_on = game.weapon_light && player.aiming && inv.usp && inv.weapon == 1;
    // Snake open to the rifle butt (the discovery's and the alert's):
    // `DAT_001ecd40 & 0x620`
    // clear (0x20 prone, `FUN_00269290`; 0x400 thrown down or lying,
    // `FUN_00285690` / `FUN_00285b28`; 0x200 hit this frame, not modelled).
    let snake_open = !player.prone && !matches!(player.react, Some(crate::player::React { stage: crate::player::ReactStage::Blown | crate::player::ReactStage::Down, .. }));
    // The frame's loudest noise: levels 5, 6 and 9 send the nearest soldier
    // in earshot (twice as far for 9) to look, if he is within 3 m of its
    // height (`FUN_0021f958`).
    let noise_now = NOISE.lock().ok().and_then(|mut n| n.take());
    if let Ok(mut h) = HEARD.lock() {
        *h = noise_now;
    }
    // An explosion (level 11): the manager raises the meter 60 frames later
    // if any soldier can still act (`FUN_0021ff20`).
    if noise_now.is_some_and(|(l, _)| l == 11) && phase.blast == 0 {
        phase.blast = 60;
    }
    let heard = noise_now.filter(|(l, _)| matches!(l, 5 | 6 | 9)).and_then(|(level, at)| {
        let reach = |g: &Guard| if level == 9 { g.hearing * 2.0 } else { g.hearing };
        guards
            .iter()
            .filter(|(_, g, b)| !g.asleep() && (b.pos.y - at.y).abs() <= 3000.0 && b.pos.distance(at) < reach(g))
            .min_by(|a, b| a.2.pos.distance(at).total_cmp(&b.2.pos.distance(at)))
            .map(|(e, _, _)| (e, at))
    });
    // Soldiers still able (the "What was that sound?" needs a comrade to
    // ask; the game counts his group, `FUN_0021d530`).
    let able = guards.iter().filter(|(_, g, _)| !g.asleep() && !g.inactive).count();
    let changed = std::mem::take(&mut phase.changed);
    let mut highest = 0i32;
    // MGS2_ALERT_AT=seconds: the meter full once then (testing; for stages
    // whose soldiers come only with the alert); `start-end` keeps it full
    // until `end`.
    if let Some((from, until)) = std::env::var("MGS2_ALERT_AT").ok().and_then(|s| {
        let (a, b) = s.split_once('-').unwrap_or((&s, ""));
        let a = a.parse::<f32>().ok()?;
        Some((a, b.parse::<f32>().unwrap_or(a)))
    }) {
        let now = time.elapsed_secs();
        if now >= from && (!*forced || now <= until) {
            *forced = true;
            highest = i32::from(METER_MAX);
        }
    }

    // MGS2_CAUTION_AT=seconds: the caution timer set once then, as a
    // report does (testing).
    if let Some(at) = std::env::var("MGS2_CAUTION_AT").ok().and_then(|s| s.parse::<f32>().ok())
        && time.elapsed_secs() >= at
        && tested.insert(usize::MAX - 1)
    {
        info!("test: caution");
        phase.caution = phase.caution_time;
    }

    // atcomm under `-t` 1 (w03b, `FUN_00272820`): the phase held at Alert
    // (Bs16:0x112 1, the meter 0x400), and its counters.
    let atc_n = guards.iter().filter(|(_, g, _)| g.defender && g.defend.fixed).count() as i32;
    if let Some(at) = game.atcomm.as_ref().filter(|a| a.at_once) {
        atc_frames(&mut phase.atc, at, atc_n, ticks);
        highest = i32::from(METER_MAX);
        // Beaten (the end of `FUN_00272820`): none left to come back and
        // every defender dead (+0x4f0, `FUN_00273570`) or down 120 frames
        // (+0x4ec): the `-p` proc, once.
        let beaten = guards
            .iter()
            .filter(|(_, g, _)| g.defender && g.defend.fixed)
            .all(|(_, g, _)| g.dead() || (g.asleep() && g.defend.down > 120));
        if atc_n > 0 && phase.atc.left == 0 && beaten && !phase.atc.cleared {
            phase.atc.cleared = true;
            info!("atcomm: the defence is beaten");
            if let Some(p) = at.cleared
                && let Ok(mut q) = DROPS.lock()
            {
                q.push((p, Vec::new()));
            }
        }
    }

    // atcomm's defence: Snake's path length from the front (+0x4b4) and
    // the points the defenders hold.
    let snake_far = game.atcomm.as_ref().map_or(FRONT_REACH, |at| {
        game.nav.as_ref().map_or(at.front.distance(snake_pos), |nav| nav.path_length(at.front, snake_pos, FRONT_REACH))
    });
    let mut taken: std::collections::HashSet<usize> = guards.iter().filter(|(_, g, _)| !g.asleep() && !g.dead()).filter_map(|(_, g, _)| g.defend.point).collect();
    // The defenders' order (+0x3a8 +0x39, `FUN_00275af8`): sorted by
    // +0x3a8 +0x28, taken to be the distance to Snake; the game's extra
    // weights for soldiers busy otherwise are not ported. Only the first
    // two attack from their points (`FUN_0028cbb0`).
    let defenders: Vec<(Entity, f32)> = guards
        .iter()
        .filter(|(_, g, _)| g.defender && !g.asleep() && !g.dead() && !g.inactive)
        .map(|(e, _, b)| (e, b.pos.distance(snake_pos)))
        .collect();
    let mut seen_now = false;

    let shot = gunshot.0.take();
    let difficulty = game.banks.b16(crate::gcl::DIFFICULTY);
    // MGS2_DOWN=route:s|k|d@seconds,..: that soldier falls asleep, is
    // knocked out or dies then, or as soon after as a soldier walks that
    // route (a script may hand it over a little later); `@air` as soon as
    // he starts a radio line; once each (testing).
    let downs: Vec<(usize, char, usize, bool)> = std::env::var("MGS2_DOWN")
        .ok()
        .map(|v| {
            v.split(',')
                .enumerate()
                .filter_map(|(n, e)| {
                    let (who, at) = e.split_once('@')?;
                    let (route, kind) = who.split_once(':')?;
                    // `@air`: when he starts a radio line.
                    let air = at == "air";
                    let at: f32 = if air { 0.0 } else { at.parse().ok()? };
                    let now = time.elapsed_secs();
                    (now >= at && !tested.contains(&n)).then_some((route.parse().ok()?, kind.chars().next()?, n, air))
                })
                .collect()
        })
        .unwrap_or_default();
    phase.callers = guards.iter().filter(|(_, g, _)| g.called && matches!(g.state, State::Call { .. })).map(|(_, _, b)| b.pos).collect();
    for (me, mut g, mut body) in &mut guards {
        // Out of play (message 0x106): nothing runs (`FUN_00222978` skips
        // his senses under +0x350 bit 1).
        if g.inactive {
            continue;
        }
        g.signal_now = std::mem::take(&mut g.signal);
        g.turn_head(&mut body, dt * 60.0);
        let darted = std::mem::take(&mut g.stung);
        if let Some(&(_, kind, n, _)) = downs.iter().find(|d| d.0 == g.route && (!d.3 || g.on_air.is_some())) {
            tested.insert(n);
            info!("test: guard {} down ({kind})", g.route);
            match kind {
                'k' => g.knock_out(),
                'd' => g.kill(),
                _ => g.fall_asleep(),
            }
        }
        // MGS2_ROUTE=from:to[:point]: that soldier walks route `to` from its
        // first point instead, or stands at `point` (testing; once).
        if let Some((from, to, at)) = std::env::var("MGS2_ROUTE").ok().and_then(|v| {
            let mut it = v.split(':').map(|x| x.parse::<usize>().ok());
            Some((it.next()??, it.next()??, it.next().flatten()))
        }) && g.route == from
            && tested.insert(usize::MAX)
        {
            info!("test: guard {from} takes route {to}");
            g.route = to;
            g.node = at.unwrap_or(0);
            if let Some(wp) = at.and_then(|n| waypoint(&game, to, n)) {
                body.pos = Vec3::from(wp.position);
                body.yaw = f32::from(wp.direction) * ANGLE;
                // Placed there, he arrives this frame: as if his point's
                // action were already under way.
                g.blind = point_blind(point_action(&wp), 0);
            }
            g.path.clear();
            g.state = State::Patrol;
        }
        if !matches!(g.state, State::Comrade { .. } | State::HoldupCall { .. } | State::Call { .. } | State::CheckIn { .. }) {
            g.radio_in_hand = false;
            g.radio_dead = false;
        }
        // His radio line cut short (out of the call before its end:
        // knocked down, darted, held up; `FUN_0021d0a8`).
        if let Some(id) = g.on_air
            && !matches!(g.state, State::Comrade { sub: 0xc | 0xe, .. } | State::HoldupCall { sub: 7 | 9, .. } | State::Call { sub: 6 | 7 | 0x1e, .. } | State::CheckIn { sub: 0, .. })
        {
            g.on_air = None;
            phase.cut(id, g.route);
        }
        let was_down = g.asleep();
        g.drowse(ticks, difficulty);
        if !was_down && g.asleep() {
            crate::sfx::play(crate::sfx::GUARD_ASLEEP + g.voice(), Some(body.pos));
        }
        // Going down (asleep, knocked out or killed) drops his `-q` item
        // (`FUN_00261a40` / `FUN_00262048` / `FUN_0024ae78`).
        if g.asleep() {
            if down.insert(me) {
                try_drop(&mut g, 0, 1.0, body.pos, &mut rng);
            }
        } else {
            down.remove(&me);
        }
        if g.defender && g.defend.fixed
            && let Some(at) = game.atcomm.as_ref().filter(|a| a.at_once)
        {
            come_back(&mut g, &mut body, at, &mut phase.atc, atc_n, ticks, &mut commands, me, &game);
        }
        if let State::Dead { timer } = g.state {
            g.level = 0;
            g.meter = 0;
            g.tracking = None;
            let frames = dt * 60.0;
            if timer < sleep::FALL_FRAMES {
                body.play_once(g.anim.fall);
                if timer + frames >= sleep::FALL_FRAMES {
                    crate::sfx::play(crate::sfx::BODY_FALL, Some(body.pos));
                }
            } else {
                body.play(g.anim.lie);
            }
            g.state = State::Dead { timer: timer + frames };
            continue;
        }
        if let State::Asleep { timer, wake } = g.state {
            let frames = dt * 60.0;
            g.level = 0;
            g.meter = 0;
            g.tracking = None;
            if timer < wake {
                if timer < sleep::FALL_FRAMES {
                    body.play_once(g.anim.fall);
                    if timer + frames >= sleep::FALL_FRAMES {
                        crate::sfx::play(crate::sfx::BODY_FALL, Some(body.pos));
                    }
                } else {
                    body.play(g.anim.lie);
                }
                g.state = State::Asleep { timer: timer + frames, wake };
            } else if let Some((motion, length)) = g.anim.get_up.map(|(slept, roused)| if g.roused { roused } else { slept })
                && timer < wake + length
            {
                // Getting up (`FUN_0024e870`).
                body.play_once(motion);
                g.state = State::Asleep { timer: timer + frames, wake };
            } else {
                g.roused = false;
                g.knocked = false;
                g.last_seen = None;
                g.stamina = g.max_stamina;
                g.resist = RESIST;
                g.darts = [0; 3];
                if let Some(wp) = waypoint(&game, g.route, g.node) {
                    plan(&mut g, body.pos, Vec3::from(wp.position), &game);
                }
                g.state = State::Return;
            }
            continue;
        }

        // Knocked down by a round: falls, gets up, then knows where Snake is.
        if let (State::Hurt { timer }, Some((fall, get_up))) = (g.state.clone(), g.anim.knocked) {
            g.level = 0;
            let t = timer + dt * 60.0;
            body.play_once(if t < hurt::FALL_FRAMES { fall } else { get_up });
            g.state = if t >= hurt::FALL_FRAMES + hurt::GET_UP_FRAMES {
                g.last_seen = Some(snake_pos);
                g.meter = METER_MAX;
                State::Discovery { timer: EXCLAIM_DONE }
            } else {
                State::Hurt { timer: t }
            };
            highest = highest.max(i32::from(g.meter));
            continue;
        }
        // Hold-ups: Snake's gun on an unaware guard at close range.
        let to_guard = Vec2::new(body.pos.x - snake_pos.x, body.pos.z - snake_pos.z);
        let snake_fwd = snake_body.forward();
        let covered = player.aiming
            && to_guard.length() < HOLDUP_RANGE
            && (body.pos.y - snake_pos.y).abs() < 1000.0
            && to_guard.normalize_or_zero().angle_to(Vec2::new(snake_fwd.x, snake_fwd.z)).abs() <= HOLDUP_ANGLE;
        if let State::HeldUp { loose, shake } = g.state {
            let frames = dt * 60.0;
            g.level = 0;
            g.meter = 0;
            body.play(g.anim.hands_up);
            let guard_fwd = body.forward();
            let in_front = (-to_guard).normalize_or_zero().angle_to(Vec2::new(guard_fwd.x, guard_fwd.z)).abs() < 1.0;
            g.state = if covered {
                let shake = if in_front { shake + frames } else { 0.0 };
                // Each further shake after his tags: his `-a` item
                // (`FUN_0023acd8`).
                if g.tagged && (shake / TAG_FRAMES).floor() > ((shake - frames) / TAG_FRAMES).floor() {
                    try_drop(&mut g, 1, 1.0, body.pos, &mut rng);
                }
                if shake >= TAG_FRAMES && !g.tagged {
                    g.tagged = true;
                    // Each soldier's tags once (the game keeps which tags
                    // are taken; `-j`'s proc skips a tag already had).
                    let key = (game.data.name.clone(), DOG_TAG_KEY | g.route as u32);
                    if inv.taken.insert(key) {
                        *inv.counts.entry(crate::items::DOG_TAGS).or_default() += 1;
                        inv.message = Some(("Dog Tags".into(), 2.5));
                    }
                }
                State::HeldUp { loose: 0.0, shake }
            } else if loose + frames > HOLDUP_LOOSE {
                g.last_seen = Some(snake_pos);
                State::HoldupCall { sub: 0x11, timer: 0, to: snake_pos }
            } else {
                State::HeldUp { loose: loose + frames, shake }
            };
            continue;
        }
        if covered
            && phase.kind != PhaseKind::Alert
            && g.level < 3
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. })
        {
            g.path.clear();
            g.state = State::HeldUp { loose: 0.0, shake: 0.0 };
            continue;
        }

        // Sensing (FUN_00222978): perception, then the meter.
        // In first person the soldiers see Snake at three quarters of their
        // ranges (`FUN_002559C0`, camera mode 6).
        let sight = if first_person.active { 0.75 } else { 1.0 };
        // Body flag 0x1000 (set by his route point's action step, which the
        // body runs before the sight reads it): nothing is seen.
        g.level = if std::mem::take(&mut g.blind) { 0 } else { perceive(&g, &body, snake_pos, snake_height, &game, sight) };
        seen_now |= g.level == 3;
        // Part of him showing past cover (stimulus 0x4000, `FUN_00230dd8`
        // tests his joints): head or feet in clear view within the near
        // range draws a look (state 6 says 0x140).
        if g.level == 1
            && !snake_dead
            && !player.boxed
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return)
            && body.pos.distance(snake_pos) <= g.vision.distance * sight
            && [snake_height * 1.5, snake_height * 0.15].iter().any(|h| perceive(&g, &body, snake_pos, *h, &game, sight) == 3)
        {
            g.state = State::Investigate { target: snake_pos, sub: 0, timer: 0, asks: true };
        }
        // The USP's light (stimulus 0x2000, `FUN_00230b40`): while Snake
        // aims it where the script allows it (`DAT_001f857c`, which the
        // weapon manager `FUN_0033ca68` sets while he aims the USP after
        // `command #f6ada8`: w03a / w03b), a soldier within
        // three times his near range notices it if, within twice that,
        // Snake points it his way (within 0x200) or else it is within his
        // own field of view, with a clear line (`FUN_00252e28` ->
        // `FUN_0014f230`); he goes to look where Snake is (normal state 6,
        // `FUN_0023c568` -> `FUN_002571d8` on the target).
        if light_on
            && !snake_dead
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return)
            && g.level < 2
        {
            let to = snake_pos - body.pos;
            let dist = to.length();
            let near = g.vision.distance;
            let bearing = to_units(to.x.atan2(to.z));
            let pointed = dist < 2.0 * near && angle_diff(to_units(snake_body.yaw), (bearing + 0x800) & 0xFFF).abs() < 0x200;
            let in_view = angle_diff(to_units(body.yaw + g.head * ANGLE), bearing).abs() < g.vision.half_angle;
            let eye = body.pos + Vec3::Y * EYE;
            if dist < 3.0 * near
                && (pointed || in_view)
                && !game.collision.as_ref().is_some_and(|c| c.line_blocked(eye, snake_pos + Vec3::Y * (snake_height * 0.6)))
            {
                info!("guard {} sees the USP's light", g.route);
                g.state = State::Investigate { target: snake_pos, sub: 0, timer: 0, asks: true };
            }
        }
        // In the box: unseen while still, only suspicious while moving.
        if player.boxed {
            g.level = if player.speed < 1.0 { 0 } else { g.level.min(1) };
        }
        // Stimulus 0x20000 (`FUN_002311f0`): the box within 1200 x 1500 x
        // 1200 mm, in the clear, and dead ahead (200/4096) or within 800 mm.
        if player.boxed
            && !snake_dead
            && phase.kind != PhaseKind::Alert
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. } | State::Search { .. } | State::Stung { .. })
        {
            let d = snake_pos - body.pos;
            let inside = |r: f32| d.x.abs() <= r && d.z.abs() <= r && d.y.abs() <= 1500.0;
            let ahead = (d.x.atan2(d.z) - body.yaw).rem_euclid(TAU);
            let ahead = ahead.min(TAU - ahead) < 200.0 * ANGLE;
            let clear = !game.collision.as_ref().is_some_and(|c| c.line_blocked(body.pos + Vec3::Y * 500.0, snake_pos + Vec3::Y * 500.0));
            if inside(1200.0) && clear && (ahead || inside(800.0)) {
                info!("guard {} checks the box", g.route);
                g.path.clear();
                g.state = State::BoxCheck { timer: 0 };
            }
        }
        if g.level == 3 {
            g.last_seen = Some(snake_pos);
            phase.last_seen = Some(snake_pos);
            g.tracking = Some(0);
        }
        for _ in 0..ticks {
            update_meter(&mut g, phase.kind);
        }
        // A comrade down in sight within his near range (stimulus 0x40,
        // `FUN_00230a48` / `FUN_0021e998`; not in an alert, which runs
        // `FUN_00231380` instead): normal state 2 (`FUN_00233330`).
        if phase.kind != PhaseKind::Alert
            && g.level < 3
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. })
        {
            let seen = sleepers.iter().find(|(e, p, _)| {
                *e != me && !attended.contains(e) && p.distance(body.pos) < g.vision.distance && perceive(&g, &body, *p, 400.0, &game, 1.0) == 3
            });
            if let Some(&(target, at, _)) = seen {
                g.path.clear();
                g.state = State::Comrade { target, at, sub: 5, timer: 0 };
            }
        }
        if let State::CheckIn { sub, timer } = g.state {
            if g.level == 3 {
                // Seen meanwhile: the normal transitions still run
                // (`FUN_00227520`), taken as discovery.
                g.radio_in_hand = false;
                g.state = State::Discovery { timer: 0 };
            } else {
                check_in(&mut g, &mut body, &mut phase, (sub, timer), dt, ticks, &game);
                highest = highest.max(i32::from(g.meter));
                continue;
            }
        }
        if let State::HoldupCall { sub, timer, to } = g.state {
            if g.level == 3 {
                // `FUN_0023b580` after its sub-state: seen, he leaves the
                // state (+0x274 = 0x12; taken as discovery).
                g.radio_in_hand = false;
                g.state = State::Discovery { timer: 0 };
            } else {
                if holdup_call(&mut g, &mut body, &mut phase, snake_pos, (sub, timer, to), dt, ticks, &game) {
                    info!("guard {} radios in the hold-up", g.route);
                    phase.caution = phase.caution_time;
                }
                highest = highest.max(i32::from(g.meter));
                continue;
            }
        }
        if let State::Comrade { target, at, sub, timer } = g.state {
            if g.level == 3 {
                g.radio_in_hand = false;
                g.state = State::Discovery { timer: 0 };
            } else {
                let down = sleepers.iter().find(|s| s.0 == target).map(|s| s.2);
                match comrade(&mut g, &mut body, &mut phase, (target, at, sub, timer), down, dt, ticks, &game) {
                    Some(Seen::Kick) => woken.push((target, 20.0)),
                    Some(Seen::Shake) => woken.push((target, 0.0)),
                    Some(Seen::Body) => seen_to.push(target),
                    Some(Seen::Report) => {
                        // `FUN_0021ee90(-h)`: the caution timer, which the
                        // manager turns into Caution.
                        info!("guard {} radios in a downed comrade", g.route);
                        phase.caution = phase.caution_time;
                    }
                    None => {}
                }
                highest = highest.max(i32::from(g.meter));
                continue;
            }
        }
        // An unsuppressed shot (noise 10) is heard as an attack: stimulus
        // 0x800 (`FUN_00230830`), straight to discovery (`FUN_00227520` ->
        // `FUN_00238f68`). Every soldier hears it within twice his hearing
        // range, in a straight line (`FUN_0021e7a8`, the level 8 / 10 case:
        // `FUN_0022de78` against +0x24c, doubled for 10; 1000 mm under `-s`
        // bit 0x40, which no Tanker soldier has; only 3 m up or down under
        // body +0xf2c bit 0x200000, not traced: taken as clear).
        if let Some(at) = shot
            && g.level < 2
            && body.pos.distance(at) <= g.hearing * 2.0
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. })
        {
            g.last_seen = Some(at);
            // Its sub-state 3 (`FUN_00237918`): no "!" line, a turn,
            // then the call ([`call`]).
            g.path.clear();
            g.called = false;
            g.state = if g.anim.check.is_some() {
                crate::sfx::play(0x53, Some(body.pos));
                State::Call { sub: 3, timer: 0, line: LINE_SIGHTED, to: Some(at) }
            } else {
                State::Discovery { timer: 0 }
            };
        }
        // Footprints in clear view within his near range: he goes to the
        // newest he sees and leaves the older ones (`FUN_00230948`,
        // `FUN_00240858`); the next ones then lead him on.
        if g.level < 2
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. } | State::Search { .. } | State::Stung { .. })
            && let Ok(mut trail) = crate::footprint::TRAIL.lock()
            && let Some(i) = trail.iter().rposition(|(at, used)| !used && perceive(&g, &body, *at, PRINT_SIGHT, &game, 1.0) == 3)
        {
            let at = trail[i].0;
            for p in trail.iter_mut().take(i + 1) {
                p.1 = true;
            }
            info!("guard {} follows footprints to {at:?}", g.route);
            stimulus(&mut g, &body, at, &game);
        }
        // A noise he is the nearest to (footsteps on a noisy floor, rounds
        // striking).
        if let Some((_, at)) = heard.filter(|(e, _)| *e == me)
            && g.level < 2
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. } | State::Search { .. } | State::Stung { .. })
        {
            info!("guard {} heard a noise at {at:?}", g.route);
            // Heard again while already looking into one (normal state 3,
            // `FUN_00233b20` on +0x286 > 0): "What was that sound?"
            // (0xc040013; 0xc040014 in Evasion), with a comrade about.
            if matches!(g.state, State::Investigate { .. } | State::Search { .. }) && able >= 2 {
                let line = if phase.kind == PhaseKind::Evasion { LINE_SOUND_EVASION } else { LINE_SOUND };
                phase.speak(line, &g);
            }
            stimulus(&mut g, &body, at, &game);
        }
        // A comrade's "!" (noise 8, `FUN_00237128`): every soldier within
        // his hearing range (`FUN_0021e7a8`, straight distance to +0x24c)
        // hears it as stimulus 1 and looks into it (normal state 3); heard
        // again while at it, the line is "What's going on!?" (0xc040020).
        if let Some((8, at)) = noise_now
            && g.level < 2
            && body.pos.distance(at) <= g.hearing
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. } | State::Search { .. } | State::Stung { .. })
        {
            info!("guard {} heard a comrade's call at {at:?}", g.route);
            if matches!(g.state, State::Investigate { .. } | State::Search { .. }) && able >= 2 {
                phase.speak(LINE_WHATS_GOING, &g);
            }
            stimulus(&mut g, &body, at, &game);
        }

        // Brain switches with the phase (inits FUN_00227E00/229F98/22C978).
        if changed {
            match phase.kind {
                PhaseKind::Alert => g.state = State::Chase,
                PhaseKind::Evasion => {
                    let to = phase.last_seen.unwrap_or(snake_pos);
                    plan(&mut g, body.pos, to, &game);
                    g.state = State::Search { wait: 0, facing: body.yaw };
                }
                PhaseKind::Caution | PhaseKind::Infiltration => {
                    if matches!(g.state, State::Chase | State::Search { .. }) {
                        if let Some(wp) = waypoint(&game, g.route, g.node) {
                            plan(&mut g, body.pos, Vec3::from(wp.position), &game);
                        }
                        g.state = State::Return;
                    }
                }
            }
        }

        // The discovery's call for backup (any phase but Alert, whose brain
        // replaces the discovery).
        if let State::Call { sub, timer, line, to } = g.state
            && phase.kind != PhaseKind::Alert
        {
            let c = call(&mut g, &mut body, &mut phase, (snake_pos, snake_open), (sub, timer, line, to), dt, ticks, &game, &mut rng);
            if c.fired {
                let muzzle = body.pos + Vec3::Y * MUZZLE + body.forward() * 400.0;
                let at = snake_pos + Vec3::Y * (snake_height * 0.6);
                bullet::fire(&mut commands, &mut rng, muzzle, at);
                crate::sfx::play(crate::sfx::USP, Some(body.pos));
            }
            if c.alert {
                g.meter = METER_MAX;
            }
            if c.caution {
                info!("guard {} reported Snake in person", g.route);
                phase.caution = phase.caution_time;
            }
            highest = highest.max(i32::from(g.meter));
            continue;
        }
        // A dart that left him awake (`FUN_00227520`, stimulus 0x4200000;
        // Evasion's `FUN_0022c168` too, its state 5; in state 7 itself only
        // after its turn, `FUN_0023cb18`).
        if darted
            && phase.kind != PhaseKind::Alert
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Search { .. } | State::Stung { sub: 2, .. })
        {
            info!("guard {} feels a dart", g.route);
            g.path.clear();
            g.state = State::Stung { at: snake_pos, sub: 1, timer: 0 };
        }
        // The defence is for the alert only (the phase's other brains,
        // `FUN_002735b0`, leave the unit alone; taken to start afresh).
        if g.defend.joined && phase.kind != PhaseKind::Alert {
            if let Some(i) = g.defend.point {
                taken.remove(&i);
            }
            g.defend = Defend { fixed: g.defend.fixed, index: g.defend.index, ..Default::default() };
        }
        match phase.kind {
            PhaseKind::Alert => {
                let rank = defenders.iter().find(|d| d.0 == me).map_or(0, |&(_, mine)| defenders.iter().filter(|d| d.0 != me && d.1 < mine).count());
                let goal = if g.defender { {
                    let ph = &mut *phase;
                    defend(&mut g, &mut body, &game, snake_pos, snake_far, rank, &mut ph.unseen, &mut ph.atc, &mut taken, ticks, &mut rng)
                } } else { None };
                if alert_brain(&mut g, &mut body, snake_pos, (snake_dead, snake_open), goal, dt, ticks, &game, &mut rng) {
                    let muzzle = body.pos + Vec3::Y * MUZZLE + body.forward() * 400.0;
                    let at = snake_pos + Vec3::Y * (snake_height * 0.6);
                    bullet::fire(&mut commands, &mut rng, muzzle, at);
                    crate::sfx::play(crate::sfx::USP, Some(body.pos));
                }
            }
            PhaseKind::Evasion => {
                // The evasion brain has the discovery state too (FUN_0022C670
                // case 2 -> FUN_00238BF8): seeing Snake again starts it.
                if g.level == 3 && !matches!(g.state, State::Discovery { .. }) {
                    g.state = State::Discovery { timer: 0 };
                }
                if matches!(g.state, State::Discovery { .. }) {
                    normal_brain(&mut g, &mut body, &mut phase, snake_pos, player.boxed, dt, ticks, &game);
                } else if let State::Stung { at, sub, timer } = g.state {
                    // Evasion state 5 (the same `FUN_0023cb78`); its end
                    // goes back to the search.
                    stung(&mut g, &mut body, (at, sub, timer), ticks, &game);
                    if g.state == State::Return {
                        g.path.clear();
                        g.state = State::Search { wait: 0, facing: body.yaw };
                    }
                } else {
                    evasion_brain(&mut g, &mut body, &phase, dt, ticks, &game);
                }
            }
            _ => normal_brain(&mut g, &mut body, &mut phase, snake_pos, player.boxed, dt, ticks, &game),
        }
        highest = highest.max(i32::from(g.meter));
    }
    phase.unseen = if seen_now { 0 } else { phase.unseen.saturating_add(ticks) };

    // Kicked awake: up in 20 frames (`FUN_00232938` sets his down timer
    // +0xf1a to 0x14); shaken: up at once (the shake is a hit of type
    // 0x1001000000000000 on him, `FUN_00254580`; that it wakes him is
    // assumed).
    for (e, left) in woken {
        if let Ok((_, mut g, _)) = guards.get_mut(e)
            && let State::Asleep { timer, wake } = g.state
            && timer < wake
        {
            g.roused = true;
            g.state = State::Asleep { timer: timer.max(wake - left), wake };
        }
    }
    for e in seen_to {
        if let Ok((_, mut g, _)) = guards.get_mut(e) {
            g.checked = true;
        }
    }

    let able = guards.iter().any(|(_, g, _)| !g.asleep());
    for _ in 0..ticks {
        let mut highest = highest;
        if phase.blast > 0 {
            phase.blast -= 1;
            if phase.blast == 0 && able {
                highest = highest.max(i32::from(METER_MAX));
            }
        }
        manager(&mut phase, highest);
        phase.no_answer_tick();
        phase.on_air = phase.on_air.and_then(|(p, f)| (f > 1).then_some((p, f - 1)));
    }
}

/// How a comrade is down: +0xffa 1 (the darts), 2 (a blow: stamina
/// gone), or dead (taken as the +0xf98 bit 0x4000000000000 the state
/// machine tests last; not confirmed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Down {
    Asleep,
    KnockedOut,
    Dead,
}

/// What seeing to a comrade does outside the soldier.
enum Seen {
    /// Kicked: he wakes 20 frames later.
    Kick,
    /// Shaken: he wakes.
    Shake,
    /// A body seen to (+0x34c bit 4): no one comes for it again.
    Body,
    /// The radio report: the caution timer runs.
    Report,
}

/// Frames the radio report takes when its line cannot play (else the
/// line's length from the manager's `-v` table, see `Radio`).
const REPORT_FRAMES: u32 = 0xF0;
/// The same fallback (0x3c) for the "it was nothing" line 0xc000012.
const COMRADE_UP_FRAMES: u32 = 0x3C;
/// He stops this close to the comrade (0x513 mm, `FUN_00232660`).
const COMRADE_REACH: f32 = 1299.0;

fn motion_length(game: &Game, body: &Body, motion: usize, fallback: u32) -> u32 {
    body.motions
        .and_then(|m| game.data.motions.get(m))
        .and_then(|(_, mar)| mar.motions.get(motion))
        .map_or(fallback, |m| m.length.max(1))
}

/// The turn motions to his right / left (`FUN_0025e7b8`: 0xd for a
/// positive turn, 0xe for a negative one; past 0x440 the rest is added
/// to the yaw at once, and at the motion's end he takes the heading).
const TURN_RIGHT: usize = 0xD;
const TURN_LEFT: usize = 0xE;

/// Normal state 7 (`FUN_0023cb78`, entered by `FUN_0023cc58` on stimulus
/// 0x200000, which `FUN_00230c20` sets for body +0xf98 bits 0x8800000000:
/// the dart handler `FUN_0024f490` sets 0x800000000 for a dart that leaves
/// him awake). The init aims him at Snake's place then (`FUN_002572a0`,
/// 0x1ece00) and goes to sub 1 (to 3 / 4 when +0x270 is 1 / 2, set by the
/// hit reactions in `FUN_00251cc8`: not ported):
/// - 1 (`FUN_0023c7f0`): on frame 6 a mark (`FUN_00253f50(.., 0xc,
///   0x8004)`, kind 4: its sprite is not identified, so none is drawn) and
///   his radio line cut (`FUN_0021d1f8`); from frame 0x3d the turn (action
///   3, `FUN_0025e7b8`) to that place, then 2;
/// - 2 (`FUN_0023c890`): the head turns +0x200 on frames 0x3d-0x77 and
///   -0x200 on 0x79-0xb3, mark kind 5 at 0xa0, done at 0xf0 (sub 5, which
///   lets `FUN_00227888` put him back on his route, `FUN_00227e00`).
///
/// Another such dart in sub 2 starts it again (`FUN_0023cb18`); noises,
/// the meter and sight still take him out of it (`FUN_00226798`).
/// Assumed: the idle motion for the action-0 wait and the look about.
fn stung(g: &mut Guard, body: &mut Body, (at, sub, timer): (Vec3, u8, u32), ticks: u32, game: &Game) {
    let t = timer + ticks;
    g.state = match sub {
        1 if t <= 0x3D => {
            body.play(g.anim.idle);
            State::Stung { at, sub, timer: t }
        }
        1 => {
            let to = at - body.pos;
            let heading = to.x.atan2(to.z);
            let mut diff = (heading - body.yaw).rem_euclid(TAU);
            if diff > PI {
                diff -= TAU;
            }
            let motion = if diff > 0.0 { TURN_RIGHT } else { TURN_LEFT };
            body.play_once(motion);
            let len = motion_length(game, body, motion, 30);
            let into = timer.saturating_sub(0x3D);
            if into + ticks >= len {
                body.yaw = heading;
                State::Stung { at, sub: 2, timer: 0 }
            } else {
                // The motion carries him round; spread the turn over it.
                let left = (len - into) as f32;
                body.yaw = turn_toward(body.yaw, heading, diff.abs() * ticks as f32 / left);
                State::Stung { at, sub, timer: t }
            }
        }
        _ => {
            body.play(g.anim.idle);
            if (0x3D..=0x77).contains(&timer) {
                g.head_to = 512.0;
            } else if (0x79..=0xB3).contains(&timer) {
                g.head_to = -512.0;
            }
            if t >= 0xF0 {
                info!("guard {} looked about after the dart", g.route);
                if let Some(wp) = waypoint(game, g.route, g.node) {
                    plan(g, body.pos, Vec3::from(wp.position), game);
                }
                State::Return
            } else {
                State::Stung { at, sub, timer: t }
            }
        }
    };
}

/// Normal state 2 (`FUN_00233120`, entered by `FUN_00233330`): a comrade
/// down in sight. Sub-states (+0x330) as in the game:
/// - 5 (`FUN_002325e8`, mark kind 4): faces him for 61 frames;
/// - 6 (`FUN_00232660`): runs to him (pace +0x254 = 3), to 1300 mm; then
///   knocked out -> 0x10, asleep -> 0x12, dead -> 0x17, up again -> 8;
/// - 0x10 (`FUN_00232a60`): waits up to 91 frames for him to come round
///   (then, in Infiltration, 0x15: a line, 0xc000012), else 7;
/// - 7 (`FUN_00232b20`): kneels and shakes him (action 8), then 8;
/// - 0x12 (`FUN_00232938`, "!"): at frame 60 kicks him (action 9); he
///   wakes 20 frames later; in Evasion that is all, else the report;
/// - 0x17 (`FUN_00232e60`, mark kind 5): looks the body over for 181
///   frames and marks it seen to, then 8;
/// - 8 (`FUN_00232cb0`): keeps looking at him until frame 181;
/// - the report (`FUN_00231c28(0x1c00000b, 0x40)`): with the caution
///   timer still over 3/4 of `-h` or in Evasion (`FUN_00252708`) only a
///   "?" for 121 frames (0xf, `FUN_00232128`); else 10 (`FUN_00231d08`,
///   action 4) takes the radio out, 0xc (`FUN_00231d88`) talks and at the
///   line's end sets the caution timer to `-h` (`FUN_0021ee90`) and puts
///   the radio away (action 5), and the state ends when that motion does.
///
/// Not ported: sub-states 0-4 (a comrade already seen to or up again:
/// "!", line 0x14c, a look; `FUN_002321b8`..`FUN_00232398`), 9 (another
/// soldier nearby who heard, `FUN_00252820`), 0x18 (he cannot see the
/// comrade from beside him: looks around, line 0x144), 0xb / 0xd (no
/// report when his radio is shot, +0xb4 bit 0x10000, or with
/// `DAT_0017f48c` bit 0x10), and the marks of kinds 4 and 5 (sprites not
/// identified).
#[allow(clippy::too_many_arguments)]
fn comrade(g: &mut Guard, body: &mut Body, phase: &mut Phase, (target, at, sub, timer): (Entity, Vec3, u8, u32), down: Option<Down>, dt: f32, ticks: u32, game: &Game) -> Option<Seen> {
    let t = timer.saturating_add(ticks);
    let reached = |n: u32| timer < n && t >= n;
    let check = g.anim.check;
    let mut out = None;
    // (sub-state, timer) to go on with; None ends the state.
    let mut next = Some((sub, t));
    match sub {
        5 => {
            body.play(g.anim.idle);
            face(body, at, dt);
            if t > 0x3c {
                plan(g, body.pos, at, game);
                next = Some((6, 0));
            }
        }
        6 => {
            body.play(g.anim.run);
            let arrived = body.pos.xz().distance(at.xz()) <= COMRADE_REACH || follow(g, body, RUN_SPEED, COMRADE_REACH, dt, game);
            if arrived || g.stuck > 2.0 {
                g.path.clear();
                next = Some((
                    match down {
                        Some(Down::KnockedOut) => 0x10,
                        Some(Down::Asleep) => 0x12,
                        Some(Down::Dead) => 0x17,
                        None => 8,
                    },
                    0,
                ));
            }
        }
        0x10 => {
            body.play(g.anim.idle);
            face(body, at, dt);
            if down.is_none() {
                next = Some((if phase.kind == PhaseKind::Infiltration { 0x15 } else { 8 }, 0));
            } else if t >= 0x5b {
                next = Some((if check.is_some() { 7 } else { 8 }, 0));
            }
        }
        0x15 => {
            // `FUN_00232c08`: the line on the first frame.
            body.play(g.anim.idle);
            if timer == 0 && t > 0 {
                phase.speak(LINE_ALL_CLEAR, g);
            }
            if t > phase.line_frames(LINE_ALL_CLEAR, g, COMRADE_UP_FRAMES) {
                next = None;
            }
        }
        7 => {
            let shake = check.map_or(g.anim.idle, |c| c.shake);
            body.play_once(shake);
            face(body, at, dt);
            if reached(22) && down.is_some() {
                out = Some(Seen::Shake);
            }
            if t >= motion_length(game, body, shake, 89) {
                next = Some((8, 0));
            }
        }
        0x12 => {
            face(body, at, dt);
            match check.filter(|_| t >= 0x3c) {
                Some(c) => {
                    body.play_once(c.kick);
                    if t >= 0x3c + motion_length(game, body, c.kick, 240) {
                        if down == Some(Down::Asleep) {
                            out = Some(Seen::Kick);
                        }
                        next = if phase.kind == PhaseKind::Evasion { None } else { Some(report(g, phase)) };
                    }
                }
                None => {
                    body.play(g.anim.idle);
                    if t >= 0x3c {
                        next = Some((8, 0));
                    }
                }
            }
        }
        0x17 => {
            body.play(g.anim.idle);
            face(body, at, dt);
            if t > 0xb4 {
                out = Some(Seen::Body);
                next = Some((8, 0));
            }
        }
        8 => {
            body.play(g.anim.idle);
            if t < 0x78 {
                face(body, at, dt);
            }
            if t > 0xb4 {
                next = None;
            }
        }
        0xf => {
            body.play(g.anim.idle);
            if t > 0x78 {
                next = None;
            }
        }
        10 => {
            let Some(c) = check else { return None };
            if radio_out(g, body, c, t, game) {
                // `FUN_00231d08`: a jammed radio -> 0xb.
                next = Some((if no_radio(g, phase) { 0xb } else { 0xc }, 0));
            }
        }
        0xc => {
            // `FUN_00231d88`: the report; at its end the caution timer.
            let Some(c) = check else { return None };
            if timer > 0 && t < phase.line_frames(LINE_REPORT, g, REPORT_FRAMES) && no_radio(g, phase) {
                cut_short(g, phase);
                next = Some((0xb, 0));
            } else {
                let (ended, away) = radio_talk(g, body, phase, c, LINE_REPORT, (timer, t), game);
                if ended {
                    out = Some(Seen::Report);
                }
                if away {
                    next = None;
                }
            }
        }
        0xb => {
            // `FUN_00231f20`: no call (action 6), then 0xd.
            let Some(c) = check else { return None };
            if radio_fails(g, body, c, t, game) {
                next = Some((0xd, 0));
            }
        }
        0xd => {
            // `FUN_00231fb8`: to the report point, then 0xe (the line is
            // still pending, +0x310).
            if run_to_report(g, body, phase, (timer, t), dt, game) {
                next = Some((0xe, 0));
            }
        }
        0xe => {
            // `FUN_00232050`: the line in person; at its end the caution
            // timer (mask 0x40 passes `FUN_00231c08`), then done.
            if say_in_person(g, body, phase, LINE_SAW_HIM, (timer, t)) {
                out = Some(Seen::Report);
                next = None;
            }
        }
        _ => next = None,
    }
    match next {
        Some((sub, timer)) => g.state = State::Comrade { target, at, sub, timer },
        None => {
            // The end (0x11): back to the route (`FUN_00227e00`).
            g.radio_in_hand = false;
            if let Some(wp) = waypoint(game, g.route, g.node) {
                plan(g, body.pos, Vec3::from(wp.position), game);
            }
            g.state = State::Return;
        }
    }
    out
}

/// Takes the radio out (action 4, `FUN_0025ebc0`): true once it is out.
fn radio_out(g: &mut Guard, body: &mut Body, c: Check, t: u32, game: &Game) -> bool {
    body.play_once(c.radio_out);
    g.radio_in_hand = t >= 28;
    t >= motion_length(game, body, c.radio_out, 99)
}

/// Talks into the radio: `line` on the first frame (`FUN_0021cdb8`), then,
/// once its frames are up, puts the radio away; HQ answers a report.
/// `(timer, t)` are the frame counts before and after this step. Returns
/// (the line just ended, the radio is away).
fn radio_talk(g: &mut Guard, body: &mut Body, phase: &mut Phase, c: Check, line: u32, (timer, t): (u32, u32), game: &Game) -> (bool, bool) {
    if timer == 0 && t > 0 {
        g.on_air = phase.speak(line, g);
    }
    let frames = phase.line_frames(line, g, REPORT_FRAMES);
    if t < frames {
        body.play(c.radio_talk);
        return (false, false);
    }
    g.on_air = None;
    body.play_once(c.radio_away);
    g.radio_in_hand = t < frames + 62;
    let ended = timer < frames && t >= frames;
    if ended && has_reply(line) {
        phase.reply();
    }
    (ended, t >= frames + motion_length(game, body, c.radio_away, 129))
}

/// Lines said in person by a soldier whose radio is dead: "I saw him! Send
/// someone over now!" (0x18a00005: state 2's sub 0xe `FUN_00232050`,
/// state 5's 9 `FUN_0023a610`) and "I'm under attack! Stay alert!"
/// (0x1c000006: the discovery's 0x1e `FUN_00237d48`; HQ answers it).
pub const LINE_SAW_HIM: u32 = 0x18A0_0005;
pub const LINE_UNDER_ATTACK: u32 = 0x1C00_0006;
/// How long the run to the report point may take (frames; not from the
/// game, whose walker `FUN_00256910` ends on arrival or a failed way).
const REPORT_RUN_FRAMES: u32 = 1800;
/// He is there within this (mm; assumed).
const REPORT_REACH: f32 = 500.0;

/// His radio cannot be used: shot (+0xb4 bit 0x10000) or jammed
/// (`DAT_0017f48c` bit 0x10), as every call checks.
fn no_radio(g: &Guard, phase: &Phase) -> bool {
    g.radio_shot || phase.jammed
}

/// The dead radio (`FUN_0025ef08`, then `FUN_0025f068`): he shakes it
/// (motion 10) and puts it away; true once it is away (+0xf58 == 1). On
/// frame 0x73 he knows it is dead (+0x348 bit 4).
fn radio_fails(g: &mut Guard, body: &mut Body, c: Check, t: u32, game: &Game) -> bool {
    if t >= 0x73 {
        g.radio_dead = true;
    }
    let shake = motion_length(game, body, c.radio_fails, 120);
    if t < shake {
        body.play_once(c.radio_fails);
        g.radio_in_hand = true;
        return false;
    }
    body.play_once(c.radio_away);
    g.radio_in_hand = t < shake + 62;
    t >= shake + motion_length(game, body, c.radio_away, 129)
}

/// A radio line cut short by jamming (`FUN_0021d0a8`): the clip stops and
/// HQ waits for an answer.
fn cut_short(g: &mut Guard, phase: &mut Phase) {
    info!("guard {}: radio dead mid-line", g.route);
    if let Some(id) = g.on_air.take() {
        phase.cut(id, g.route);
    }
}

/// Runs to the manager's `-c` point (`FUN_00257388` sets it, gait +0xf5a
/// 7 taken as the run, `FUN_00256910` walks there); true on arrival.
fn run_to_report(g: &mut Guard, body: &mut Body, phase: &Phase, (timer, t): (u32, u32), dt: f32, game: &Game) -> bool {
    if timer == 0 || g.path.is_empty() {
        plan(g, body.pos, phase.report_at, game);
    }
    let there = body.pos.xz().distance(phase.report_at.xz()) <= REPORT_REACH;
    let done = there || follow(g, body, RUN_SPEED, REPORT_REACH, dt, game) || g.stuck > 2.0 || t >= REPORT_RUN_FRAMES;
    body.play(if done { g.anim.idle } else { g.anim.run });
    if done {
        g.path.clear();
    }
    done
}

/// A line said in person (no radio out, `FUN_0021cdb8` on the first frame);
/// true on the frame it ends.
fn say_in_person(g: &mut Guard, body: &mut Body, phase: &mut Phase, line: u32, (timer, t): (u32, u32)) -> bool {
    body.play(g.anim.idle);
    if timer == 0 && t > 0 {
        info!("guard {} reports in person", g.route);
        // The in-person subs clear the shot-radio flag on entry, and his
        // limb wounds (`FUN_00255978`: +0xf16 and the counts +0xf1c-f).
        g.radio_shot = false;
        g.wounds = 0;
        g.on_air = phase.speak(line, g);
    }
    let frames = phase.line_frames(line, g, REPORT_FRAMES);
    let ended = timer < frames && t >= frames;
    if ended {
        g.on_air = None;
        if has_reply(line) {
            phase.reply();
        }
    }
    ended
}

/// Lines that `FUN_0021cb40` follows with HQ's reply.
fn has_reply(line: u32) -> bool {
    matches!(line, 0x1C00_0006..=0x1C00_0008 | 0x1C00_000B | 0x1C00_000D)
}

/// A hold-up let go (normal state 5, `FUN_0023b580`); sub-states (+0x330)
/// as in the game:
/// - 0x11 (`FUN_0023a778`, entered from 4 after 0x79 frames uncovered,
///   `FUN_0023af20`): "!", faces Snake for 9 frames, then the report
///   `FUN_0023a1e8(0x1c000007, 0x200)`:
/// - with the caution timer over 3/4 of `-h` or in Evasion
///   (`FUN_00252708`): 10 (`FUN_0023a6e8`), "?" for 121 frames;
/// - else 5 (`FUN_0023a2c8`, action 6) takes the radio out and 7
///   (`FUN_0023a348`) says the line, at whose end the caution timer runs
///   (0x200 passes `FUN_0023a1c8`) and the radio goes away (action 7);
/// - jammed (`DAT_0017f48c` bit 0x10) when the radio is out, or during the
///   line (cut short): 6 (`FUN_0023a4e0`, action 8: the dead radio), 8
///   (`FUN_0023a578`, a run to the manager's `-c` point) and 9
///   (`FUN_0023a610`: "I saw him! Send someone over now!" in person, then
///   the caution timer).
/// - 0xe: done, back to the route.
///
/// Not ported: entering 8 directly (both arms hurt, +0xf16 & 3 == 3), the
/// radio shot (+0xb4 bit 0x10000), 7 without the radio (`FUN_002527d0`,
/// action 9). State 5's actions 6 / 7 / 8 are taken as the radio motions
/// of state 2's 4 / 5 / 6 (sub 5 waits for +0xf58 == 3, the
/// radio-out motion's end signal). Returns true when the report ends.
#[allow(clippy::too_many_arguments)]
fn holdup_call(g: &mut Guard, body: &mut Body, phase: &mut Phase, snake: Vec3, (sub, timer, to): (u8, u32, Vec3), dt: f32, ticks: u32, game: &Game) -> bool {
    let t = timer.saturating_add(ticks);
    // The body keeps turning to its target heading (assumed for the
    // radio actions too: nothing here clears +0xf54).
    let to = if sub == 0x11 { snake } else { to };
    if sub != 8 {
        face(body, to, dt);
    }
    let mut reported = false;
    let mut next = Some((sub, t));
    match sub {
        0x11 => {
            body.play(g.anim.idle);
            if t >= 9 {
                let quiet = phase.kind == PhaseKind::Evasion || phase.caution > phase.caution_time / 4 * 3 || g.anim.check.is_none();
                next = Some((if quiet { 10 } else { 5 }, 0));
            }
        }
        10 => {
            body.play(g.anim.idle);
            if t > 0x78 {
                next = None;
            }
        }
        5 => {
            let Some(c) = g.anim.check else { return false };
            if radio_out(g, body, c, t, game) {
                // `FUN_0023a2c8`: a jammed radio -> 6.
                next = Some((if no_radio(g, phase) { 6 } else { 7 }, 0));
            }
        }
        7 => {
            let Some(c) = g.anim.check else { return false };
            if timer > 0 && t < phase.line_frames(LINE_HOLDUP, g, REPORT_FRAMES) && no_radio(g, phase) {
                cut_short(g, phase);
                next = Some((6, 0));
            } else {
                let (ended, away) = radio_talk(g, body, phase, c, LINE_HOLDUP, (timer, t), game);
                reported = ended;
                if away {
                    next = None;
                }
            }
        }
        6 => {
            // `FUN_0023a4e0`: the dead radio, then 8.
            let Some(c) = g.anim.check else { return false };
            if radio_fails(g, body, c, t, game) {
                next = Some((8, 0));
            }
        }
        8 => {
            // `FUN_0023a578`: to the report point, then 9.
            if run_to_report(g, body, phase, (timer, t), dt, game) {
                next = Some((9, 0));
            }
        }
        9 => {
            // `FUN_0023a610`: the line in person; at its end the caution
            // timer (mask 0x200), then 0xe.
            if say_in_person(g, body, phase, LINE_SAW_HIM, (timer, t)) {
                reported = true;
                next = None;
            }
        }
        _ => next = None,
    }
    match next {
        Some((sub, timer)) => g.state = State::HoldupCall { sub, timer, to },
        None => {
            g.radio_in_hand = false;
            if let Some(wp) = waypoint(game, g.route, g.node) {
                plan(g, body.pos, Vec3::from(wp.position), game);
            }
            g.state = State::Return;
        }
    }
    reported
}

/// The discovery's rifle butt (actions 8 / 0xb: `FUN_0025f810`, or
/// `FUN_0025fa48` with the right arm hurt, +0xf16 & 1), facing Snake, its
/// hit on the motion's frames 2-5 as the alert's; true at the motion's end.
fn call_butt(g: &mut Guard, body: &mut Body, snake: Vec3, (timer, t): (u32, u32), dt: f32, game: &Game) -> bool {
    let (motion, joint) = if g.wounds & 1 != 0 { (WOUNDED_STRIKE_MOTION, WOUNDED_STRIKE_JOINT) } else { (STRIKE_MOTION, STRIKE_JOINT) };
    body.play_once(motion);
    face(body, snake, dt);
    if timer == 0 {
        g.attack.strike = Some((0.0, false));
    }
    if let Some((_, false)) = g.attack.strike
        && (timer..t.max(timer + 1)).any(|f| STRIKE_FRAMES.contains(&(f as f32)))
        && strike_hits(game, body, joint, snake)
    {
        info!("guard {} strikes with the rifle butt", g.route);
        g.attack.strike = Some((t as f32, true));
        land_strike(body);
    }
    let done = t >= motion_length(game, body, motion, 40);
    if done {
        g.attack.strike = None;
    }
    done
}

/// What a frame of [`call`] did.
#[derive(Default)]
struct Called {
    /// A round fired at Snake.
    fired: bool,
    /// The call's line ended: the meter goes to 0x400 (`FUN_00236f30`).
    alert: bool,
    /// A report in person ended: the caution timer (`FUN_00237d48`).
    caution: bool,
}

/// How long the back-off may take before he calls anyway (frames; not
/// from the game: `FUN_00256988` ends it on arrival or when the way
/// fails).
const BACK_OFF_FRAMES: u32 = 300;

/// The discovery after its "!" (w00a table 0x3f0c20). In the Tanker no
/// soldier has `-s` bit 0x10000, and body flag 0x200000000
/// (`FUN_002527d0`) is never set (w00a and the ELF only pass 0x12000 and
/// 1 to `FUN_00247d10`), so every discovery ends in a radio call:
/// - 8 (`FUN_002381e8`, aim): with Snake out of sight on frame 5, sub
///   0x15; Snake seen within 1 m before frame 7, 0x13; from frame 7, 9
///   (0x15 after 30 rounds);
/// - 0x13 (`FUN_00237ef8`): the rifle butt (action 8, `FUN_0025f810`,
///   motion 0x55), then 0x15;
/// - 9 (`FUN_00238008`): every 4th frame a round (action 9; +0x2a0 counts
///   them), or from frame 7 a 1-in-3 chance of 0x15; after 31 rounds 0x12
///   (`FUN_00238120`, action 10: the reload, then 8);
/// - 0x15 (`FUN_00237e00`): backs off to the nav cell away from Snake
///   (`FUN_00253c38` / `FUN_00256988`), then 0xe with line 0x2c000004;
/// - 3 (`FUN_00237918`, a shot heard, entered here instead of the "!"):
///   0x1f frames, then action 2 (the "!" motion) facing the noise, then 0xe
///   with 0x2c000000 (0x2c000004 under DAT_001f39b0 bit 0, not traced);
///   0x14 with his radio known dead; 10 when a comrade within his hearing
///   has called it in (+0x344 bit 8) and no line of priority 2 or more is
///   on the air;
/// - 10 (`FUN_00237b10`): the "!" again, then the meter to 0x400 (the
///   alert) without a call;
/// - 0xe (`FUN_00237528`): the radio out (action 5), then 6 with Snake in
///   sight, else 7; 0xf when the radio is jammed (`DAT_0017f48c` bit 0x10;
///   a shot radio, +0xb4 bit 0x10000, is not ported);
/// - 6 / 7 (`FUN_002375d8` / `FUN_00237710`): the line; at its end the
///   radio goes away (action 6) and the meter goes to 0x400, which raises
///   the alert. HQ's area answer follows (`FUN_0021cb40`). Jammed during
///   the line: cut short (`FUN_0021d0a8`), 0xf;
/// - 0xf (`FUN_00237898`, action 7): the dead radio; from frame 0x73 +0x348
///   bit 4, so 8 and 9 go to 0x14 instead of 0x15 (no rounds from 8);
/// - 0x14 (`FUN_00237c90`): runs to the manager's `-c` point, then 0x1e
///   (`FUN_00237d48`): "I'm under attack! Stay alert!" in person (HQ
///   replies), the caution timer and back to the route; no alert.
///
/// - 0x20 (`FUN_00237e78`, from 0x14 with Snake seen within 1 m): the
///   rifle butt on the run, then 0x14 again.
///
/// Not ported: 0xf's 0x18, +0x348 bit 0x100 (the runner joins the manager's list of
/// soldiers to see to). The radio
/// motions are state 2's (as for the hold-up call); the back-off runs
/// (assumed).
#[allow(clippy::too_many_arguments)]
fn call(g: &mut Guard, body: &mut Body, phase: &mut Phase, (snake, open): (Vec3, bool), (sub, timer, line, to): (u8, u32, u32, Option<Vec3>), dt: f32, ticks: u32, game: &Game, rng: &mut Rng) -> Called {
    let mut out = Called::default();
    let Some(c) = g.anim.check else {
        g.meter = METER_MAX;
        g.state = State::Discovery { timer: EXCLAIM_DONE };
        return out;
    };
    let t = timer.saturating_add(ticks);
    // Frames this step covers (the game's per-frame checks).
    let frames = timer..t.max(timer + 1);
    let seen = g.level == 3;
    let mut next = (sub, t, line, to);
    match sub {
        8 => {
            body.play(g.anim.aim);
            face(body, snake, dt);
            // With the radio dead (+0x348 bit 4) he runs to report: 0x14.
            let away = if g.radio_dead { 0x14 } else { 0x15 };
            if frames.contains(&5) && !seen {
                next = (away, 0, line, None);
            } else if timer < 7 && seen && open && body.pos.distance(snake) < STRIKE_RANGE && g.wounds & 0xC == 0 {
                // Snake within 1 m on its first 7 frames: the rifle butt.
                next = (0x13, 0, line, None);
            } else if t >= 7 {
                next = (if g.radio_dead { 0x14 } else if g.attack.shots > 30 { 0x15 } else { 9 }, 0, line, None);
            }
        }
        0x13 => {
            // `FUN_00237ef8`: action 8 (`FUN_0025f810`, the alert's strike,
            // motion 0x55; 0xb with the right arm hurt), then the back-off
            // 0x15 (0x14 with the radio dead).
            if call_butt(g, body, snake, (timer, t), dt, game) {
                next = (if g.radio_dead { 0x14 } else { 0x15 }, 0, line, None);
            }
        }
        0x20 => {
            // `FUN_00237e78`: the same strike on the run, until the action
            // ends (+0xf58) or 0x79 frames; then the run again (0x14, its
            // target set anew by `FUN_00257388`).
            if call_butt(g, body, snake, (timer, t), dt, game) || t >= 0x79 {
                g.path.clear();
                next = (0x14, 0, line, None);
            }
        }
        9 => {
            body.play(g.anim.aim);
            face(body, snake, dt);
            if g.attack.shots >= MAGAZINE {
                next = (0x12, 0, line, None);
            } else if let Some(f) = frames.clone().find(|f| f % 4 == 0) {
                if f >= 7 && rng.next().is_multiple_of(3) {
                    next = (if g.radio_dead { 0x14 } else { 0x15 }, 0, line, None);
                } else {
                    g.attack.shots += 1;
                    out.fired = true;
                }
            }
        }
        0x12 => {
            body.play_once(RELOAD_MOTION);
            face(body, snake, dt);
            if t >= motion_length(game, body, RELOAD_MOTION, RELOAD_FRAMES as u32) {
                info!("guard {} reloads", g.route);
                g.attack.shots = 0;
                next = (8, 0, line, None);
            }
        }
        0x15 => {
            let to = to.or_else(|| game.nav.as_ref().and_then(|n| n.away(body.pos, snake)));
            let arrived = match to {
                Some(p) => {
                    if g.path.is_empty() || timer == 0 {
                        plan(g, body.pos, p, game);
                    }
                    let done = follow(g, body, RUN_SPEED, 300.0, dt, game);
                    body.play(if done { g.anim.idle } else { g.anim.run });
                    done
                }
                None => true,
            };
            next = if arrived || t >= BACK_OFF_FRAMES {
                g.path.clear();
                g.called = true;
                phase.callers.push(body.pos);
                (0xe, 0, LINE_ENEMY, None)
            } else {
                (0x15, t, line, to)
            };
        }
        3 => {
            let at = to.unwrap_or(snake);
            face(body, at, dt);
            if t < 0x1F {
                body.play(g.anim.idle);
            } else {
                body.play_once(g.anim.exclaim);
                if t >= 0x1F + motion_length(game, body, g.anim.exclaim, EXCLAIM_FRAMES) {
                    // With his radio known dead (+0x348 bit 4): the run to
                    // report, 0x14. A comrade within his hearing (+0x24c)
                    // who has called it in (`FUN_0021eb20(.., 8, ..)`)
                    // while no line of priority 2 or more is on the air
                    // (`FUN_0021d230`): 10, no call of his own.
                    let near = phase.callers.iter().map(|p| p.distance(body.pos)).fold(f32::INFINITY, f32::min);
                    next = if g.radio_dead {
                        (0x14, 0, line, None)
                    } else if near < g.hearing && phase.air_priority() < 2 {
                        info!("guard {} leaves the call to a comrade", g.route);
                        (10, 0, line, to)
                    } else {
                        g.called = true;
                        phase.callers.push(body.pos);
                        (0xe, 0, LINE_SIGHTED, None)
                    };
                }
            }
        }
        10 => {
            // `FUN_00237b10`: action 2 (the "!" motion, effect 0x53 again)
            // facing the noise; at its end the meter goes to 0x400 (the
            // alert) and the state's sub 0.
            let at = to.unwrap_or(snake);
            face(body, at, dt);
            if timer == 0 {
                crate::sfx::play(0x53, Some(body.pos));
            }
            body.play_once(g.anim.exclaim);
            if t >= motion_length(game, body, g.anim.exclaim, EXCLAIM_FRAMES) {
                out.alert = true;
            }
        }
        0xe => {
            if seen {
                face(body, snake, dt);
            }
            if radio_out(g, body, c, t, game) {
                next = (if no_radio(g, phase) { 0xf } else if seen { 6 } else { 7 }, 0, line, None);
            }
        }
        0xf => {
            // `FUN_00237898`: the dead radio (action 7), then 8 (0x18 under
            // +0x348 bit 0x2000, not ported).
            if radio_fails(g, body, c, t, game) {
                next = (8, 0, line, None);
            }
        }
        0x14 => {
            // `FUN_00237c90`: to the report point, then 0x1e; on the way,
            // Snake seen within 1 m and on his feet (`DAT_001ecd40` &
            // 0x620), his legs whole (+0xf16 & 0xc): the rifle butt, 0x20.
            if run_to_report(g, body, phase, (timer, t), dt, game) {
                next = (0x1e, 0, line, None);
            } else if seen && open && body.pos.distance(snake) < STRIKE_RANGE && g.wounds & 0xC == 0 {
                info!("guard {} meets Snake on his run", g.route);
                next = (0x20, 0, line, None);
            }
        }
        0x1e => {
            // `FUN_00237d48`: "I'm under attack! Stay alert!" in person; at
            // its end the caution timer and +0x348 bit 0x10000, which ends
            // the state (`FUN_00227b90`).
            if say_in_person(g, body, phase, LINE_UNDER_ATTACK, (timer, t)) {
                out.caution = true;
                g.radio_dead = false;
                if let Some(wp) = waypoint(game, g.route, g.node) {
                    plan(g, body.pos, Vec3::from(wp.position), game);
                }
                g.state = State::Return;
                return out;
            }
        }
        6 | 7 => {
            // Snake in sight: face him (both subs); sub 7 otherwise looks
            // about on frames 0x3d-0x77 (+0x200) and 0x79-0xb3 (-0x200).
            if sub == 7 && !seen && frames.contains(&0x3D) {
                info!("guard {} looks about on the radio", g.route);
            }
            if sub == 6 || seen {
                face(body, snake, dt);
            } else if (0x3D..=0x77).contains(&timer) {
                g.head_to = 512.0;
            } else if (0x79..=0xB3).contains(&timer) {
                g.head_to = -512.0;
            }
            if timer > 0 && t < phase.line_frames(line, g, REPORT_FRAMES) && no_radio(g, phase) {
                cut_short(g, phase);
                g.state = State::Call { sub: 0xf, timer: 0, line, to: None };
                return out;
            }
            let (ended, away) = radio_talk(g, body, phase, c, line, (timer, t), game);
            if ended {
                info!("guard {} radios for backup", g.route);
                out.alert = true;
                phase.area_line(snake, game, rng.next());
            }
            if away {
                g.radio_in_hand = false;
                if let Some(wp) = waypoint(game, g.route, g.node) {
                    plan(g, body.pos, Vec3::from(wp.position), game);
                }
                g.state = State::Return;
                return out;
            }
        }
        _ => next = (8, 0, line, None),
    }
    let (sub, timer, line, to) = next;
    g.state = State::Call { sub, timer, line, to };
    out
}

/// `FUN_00231c28` for the downed-comrade report: the sub-state it goes to.
fn report(g: &Guard, phase: &Phase) -> (u8, u32) {
    if phase.kind == PhaseKind::Evasion || phase.caution > phase.caution_time / 4 * 3 || g.anim.check.is_none() {
        (0xf, 0)
    } else {
        (10, 0)
    }
}

/// `FUN_00227BC8` + transitions `FUN_00227520` (states the game can
/// currently produce).
fn normal_brain(g: &mut Guard, body: &mut Body, phase: &mut Phase, snake: Vec3, boxed: bool, dt: f32, ticks: u32, game: &Game) {
    // Transitions out of patrol/wait/return, in the game's priority order
    // (later checks override earlier ones).
    // State 7 keeps these (`FUN_00227888` -> `FUN_00226798`).
    if matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Stung { .. }) {
        if g.meter > 0 {
            let target = g.last_seen.unwrap_or(snake);
            g.state = State::Investigate { target, sub: 0, timer: 0, asks: true };
        }
        if g.meter >= 2 || g.level == 3 {
            g.state = State::Discovery { timer: 0 };
        }
    }
    // Investigation escalates (FUN_00227430).
    if matches!(g.state, State::Investigate { .. }) && g.meter > 8 {
        g.state = State::Discovery { timer: 0 };
    }

    // A new route from the script (message 1, `FUN_00222810` -> +0xc0f),
    // taken up by the route walker (`FUN_00224760`: +0xc0f != +0xc0c ->
    // `FUN_00252a20`; start point +0xc10, or the nearest when none).
    if let Some((route, node)) = g.pending_route
        && matches!(g.state, State::Patrol | State::Wait(_) | State::Return)
    {
        g.pending_route = None;
        if route != g.route {
            info!("guard {} takes route {route}", g.route);
            g.route = route;
            g.node = node.unwrap_or_else(|| nearest_point(game, route, body.pos));
            if let Some(wp) = waypoint(game, g.route, g.node) {
                plan(g, body.pos, Vec3::from(wp.position), game);
            }
            g.state = State::Return;
        }
    }
    if !matches!(g.state, State::Patrol) {
        g.side_turn = 0.0;
    }
    match g.state.clone() {
        State::Patrol => {
            let Some(wp) = waypoint(game, g.route, g.node) else {
                body.play(g.anim.idle);
                return;
            };
            let (walk, walk_speed, side) = gait_motion(g, g.gait);
            body.play(walk);
            body.yaw -= std::mem::take(&mut g.side_turn);
            if g.path.is_empty() {
                g.path = vec![Vec3::from(wp.position)];
            }
            // Walking straight at the next route point got nowhere: go round
            // through the navigation graph, and failing that move on.
            if g.stuck > 1.0 {
                if g.path.len() <= 1 && game.nav.as_ref().is_some_and(|n| !n.is_empty()) {
                    plan(g, body.pos, Vec3::from(wp.position), game);
                } else {
                    g.path.clear();
                    g.node += 1;
                }
                g.stuck = 0.0;
                return;
            }
            let arrived = follow(g, body, walk_speed, 150.0, dt, game);
            if !arrived && side != 0.0 {
                body.yaw += side;
                g.side_turn = side;
            }
            if arrived {
                // At the point (`FUN_002244f0`): the manager's `-e` event
                // for it runs (`FUN_0021dc60`, the soldier's name as $1).
                let node = g.node % game.data.collision.as_ref().and_then(|c| c.routes.get(g.route)).map_or(1, |r| r.points.len().max(1));
                if let Some(proc) = phase.route_event(g.route, node) {
                    info!("guard {} at point {node}: event {proc:#x}", g.route);
                    if let Ok(mut q) = DROPS.lock() {
                        q.push((proc, vec![g.name]));
                    }
                }
                g.point_frames = 0;
                let (action, wait) = point_plan(&wp, phase);
                if action != 0 || wait < 0 {
                    info!("guard {} at point {node}: action {action:#x}, wait {wait}", g.route);
                }
                // The action starts now: its blindness from this frame.
                g.blind = action_motion(action).is_some() && point_blind(action, 0);
                let gait = pick_gait(&wp, action, phase);
                if gait != g.gait {
                    info!("guard {} at point {node}: gait {gait}", g.route);
                }
                g.gait = gait;
                // A route of one point is a post: he stands there.
                let posts = game.data.collision.as_ref().and_then(|c| c.routes.get(g.route)).is_some_and(|r| r.points.len() <= 1);
                g.state = match wait {
                    _ if posts => State::Wait(f32::INFINITY),
                    0 => {
                        g.node += 1;
                        State::Patrol
                    }
                    // `FUN_002246f8`: the manager's `-f` line for this point
                    // (`FUN_0021c9e0`, line 0x4060000 | n: no click), and he
                    // waits while he says it (0xf0 frames without one).
                    -4 => {
                        let frames = match phase.route_line(g.route, node) {
                            Some((clip, frames)) => {
                                info!("guard {} at point {node}: line {clip:#x}", g.route);
                                phase.play_line(ROUTE_LINE, clip);
                                frames
                            }
                            None => 0xF0,
                        };
                        State::Wait(frames as f32 / 60.0)
                    }
                    w if w < 0 => State::Wait(f32::INFINITY),
                    w => State::Wait(f32::from(w) / 60.0),
                };
            }
        }
        State::Wait(left) => {
            let Some(wp) = waypoint(game, g.route, g.node) else {
                body.play(g.anim.idle);
                return;
            };
            let t = g.point_frames.saturating_add(ticks);
            g.point_frames = t;
            let (action, wait) = point_plan(&wp, phase);
            // Action 0x19 (route action 23: w02a's door soldier, route 43
            // point 1): his check-in, `FUN_00224530` on each frame of the
            // wait. With his radio shot or jammed on the first frame,
            // stimulus 0x8000000: [`check_in`]. Else the radio comes out
            // (+0xf58 == 3), he says the manager's `-f` line for the point
            // (`FUN_0021c9e0(.., 0x4020000)`: with the click; the wait is
            // its length, 0xf0 frames without one) and puts the radio away
            // (action 0x1a); the -1 wait ends with that motion.
            let check = g.anim.check.filter(|_| action == 0x19);
            if let Some(c) = check {
                if t == ticks && no_radio(g, phase) {
                    info!("guard {}: no radio for the check-in", g.route);
                    g.state = State::CheckIn { sub: 1, timer: 0 };
                    return;
                }
                let out = motion_length(game, body, c.radio_out, 99);
                let node = g.node % game.data.collision.as_ref().and_then(|r| r.routes.get(g.route)).map_or(1, |r| r.points.len().max(1));
                let (clip, frames) = phase.route_line(g.route, node).unwrap_or((0, 0xF0));
                let away = motion_length(game, body, c.radio_away, 129);
                if t < out {
                    radio_out(g, body, c, t, game);
                } else if t < out + frames {
                    if t - ticks < out && clip != 0 {
                        info!("guard {} checks in: line {clip:#x}", g.route);
                        phase.play_line(CHECK_IN_LINE, clip);
                    }
                    body.play(c.radio_talk);
                    g.radio_in_hand = true;
                } else {
                    body.play_once(c.radio_away);
                    g.radio_in_hand = t < out + frames + 62;
                }
                body.yaw = turn_toward(body.yaw, f32::from(wp.direction) * ANGLE, TURN_RATE * dt);
                g.state = if t >= out + frames + away {
                    g.node += 1;
                    State::Patrol
                } else {
                    State::Wait(left)
                };
                return;
            }
            let playing = action_motion(action).filter(|&m| t < motion_length(game, body, m, 0));
            match playing {
                Some(m) => body.play_once(m),
                None => body.play(g.anim.idle),
            }
            g.blind = playing.is_some() && point_blind(action, t);
            // Action 6's voice (`FUN_00260320`): on frames 70, 226 and 393 of
            // its motion, sound 0x11c + his voice (0x36e under the body's
            // flag 0x80000 via +0xfa8, not traced).
            if action == 6 && playing.is_some() && [0x46u32, 0xE2, 0x189].iter().any(|&f| t - ticks.min(t) < f && f <= t) {
                crate::sfx::play(0x11C + g.voice(), Some(body.pos));
            }
            // He faces the point's direction (+0xc04 -> +0xf54); his head
            // looks at its look point, or swings for action 0xd.
            body.yaw = turn_toward(body.yaw, f32::from(wp.direction) * ANGLE, TURN_RATE * dt);
            if let Some(l) = wp.look_at() {
                let to = (l[0] - body.pos.x).atan2(l[2] - body.pos.z);
                g.head_to = signed_angle(to - body.yaw) / ANGLE;
            } else if action == 0xD {
                g.head_to = head_swing(t);
            }
            let left = left - dt;
            let done = match wait {
                // `FUN_00225dc8`: until the action's motion ends (+0xf58 == 1).
                -1 => action_done(action, t, game, body),
                -2 => false,
                // `FUN_002246c0`: until the script's message 2 (+0x34c bit 8).
                -3 => g.signal_now,
                _ => left <= 0.0,
            };
            g.state = if done {
                g.node += 1;
                State::Patrol
            } else {
                State::Wait(left)
            };
        }
        State::Investigate { target, sub, timer, asks } => {
            // Sub-states: 0 glance toward the target, 5 a pause and a
            // line, 1 walk there, 2 look around (sub-state 10: up to 300
            // frames), then return.
            let before = timer;
            let timer = timer + ticks;
            g.state = match sub {
                0 => {
                    body.play(g.anim.idle);
                    face(body, target, dt);
                    if timer > 30 {
                        State::Investigate { target, sub: 5, timer: 0, asks }
                    } else {
                        State::Investigate { target, sub, timer, asks }
                    }
                }
                5 => {
                    // `FUN_0023c108`: after 61 frames "Is there somebody
                    // there?" (line 0xc04001d; "Over there?" 0xc04001e in
                    // Evasion), then the walk (sub-state 8 in the game).
                    // State 3's pause (`FUN_00233b20`) says nothing the
                    // first time.
                    body.play(g.anim.idle);
                    if timer > 0x3c {
                        if asks {
                            phase.speak(if phase.kind == PhaseKind::Evasion { LINE_OVER_THERE } else { LINE_SOMEBODY }, g);
                        }
                        plan(g, body.pos, target, game);
                        State::Investigate { target, sub: 1, timer: 0, asks }
                    } else {
                        State::Investigate { target, sub, timer, asks }
                    }
                }
                1 => {
                    body.play(g.anim.walk);
                    if follow(g, body, g.anim.walk_speed, 600.0, dt, game) || timer > 600 {
                        State::Investigate { target, sub: 2, timer: 0, asks }
                    } else {
                        State::Investigate { target, sub, timer, asks }
                    }
                }
                _ => {
                    if let Some(look) = g.anim.look {
                        body.play(look);
                    } else {
                        body.play(g.anim.idle);
                        body.yaw += (timer as f32 / 60.0).sin() * dt;
                    }
                    // 120 frames after arriving: "?" and line 0x144 (w00a
                    // `FUN_0023c370`, not during Evasion).
                    if before < 120 && timer >= 120 {
                        say(0x144, g, body.pos);
                    }
                    if timer > 300 {
                        g.last_seen = None;
                        if let Some(wp) = waypoint(game, g.route, g.node) {
                            plan(g, body.pos, Vec3::from(wp.position), game);
                        }
                        State::Return
                    } else {
                        State::Investigate { target, sub, timer, asks }
                    }
                }
            };
        }
        State::Stung { at, sub, timer } => stung(g, body, (at, sub, timer), ticks, game),
        State::BoxCheck { timer } => {
            // The inspection's actions (`FUN_002413d0`) are not ported: he
            // stops, faces the box, "?", and lifts it on Snake if he is still
            // inside (assumed timing), else goes back to his route.
            body.play(g.anim.idle);
            face(body, snake, dt);
            let before = timer;
            let timer = timer + ticks;
            if before < 30 && timer >= 30 {
                say(0x144, g, body.pos);
            }
            g.state = if timer < BOX_CHECK_FRAMES {
                State::BoxCheck { timer }
            } else if boxed && body.pos.distance(snake) < 1500.0 {
                g.last_seen = Some(snake);
                State::Discovery { timer: 0 }
            } else {
                if let Some(wp) = waypoint(game, g.route, g.node) {
                    plan(g, body.pos, Vec3::from(wp.position), game);
                }
                State::Return
            };
        }
        State::Discovery { timer } => {
            // "!" (discovery sub-state 2, `FUN_00237128`): face Snake while
            // the reaction plays, then aim and call it in (sub 8, [`call`]),
            // whose end raises the meter to 0x400 and so the alert.
            body.play_once(g.anim.exclaim);
            face(body, snake, dt);
            let length = body
                .motions
                .and_then(|m| game.data.motions.get(m))
                .and_then(|(_, mar)| mar.motions.get(g.anim.exclaim))
                .map_or(EXCLAIM_FRAMES, |m| m.length.max(1));
            let before = timer;
            if before == 0 {
                // His "!" is heard by the others: noise 8 at his place
                // (`FUN_00237128`: `FUN_0011b630(8, ..)`).
                noise(8, body.pos);
            }
            let mut timer = timer.saturating_add(ticks);
            // Snake within a metre cuts it short after 7 frames
            // (`FUN_00237128`).
            if timer >= 7 && timer < length && body.pos.distance(snake) < 1000.0 {
                timer = length;
            }
            if timer >= length {
                // As it ends the guard calls out (line 300 + voice).
                if before < length {
                    crate::sfx::play(0x12C + g.voice(), Some(body.pos));
                }
                // Soldiers without the radio motions (the Marines) raise
                // the alert at once, as before.
                if g.anim.check.is_some() {
                    g.called = false;
                    g.state = State::Call { sub: 8, timer: 0, line: LINE_ENEMY, to: None };
                    return;
                }
                g.meter = METER_MAX;
            }
            g.state = State::Discovery { timer };
        }
        State::Return => {
            // Back to the route with his gait (the side steps as the walk;
            // assumed).
            let (walk, walk_speed, _) = gait_motion(g, if matches!(g.gait, 2 | 3) { 0 } else { g.gait });
            body.play(walk);
            if follow(g, body, walk_speed, 300.0, dt, game) {
                g.path.clear();
                g.state = State::Patrol;
            }
        }
        State::Chase | State::Search { .. } => g.state = State::Return,
        State::Asleep { .. } | State::Dead { .. } | State::Comrade { .. } | State::HeldUp { .. } | State::HoldupCall { .. } | State::CheckIn { .. } | State::Call { .. } | State::Hurt { .. } => {}
    }
}

/// `FUN_00229D30`: chase, and shoot when Snake is in sight. Returns true
/// when a shot is fired this frame.
#[allow(clippy::too_many_arguments)]
/// The defenders' hold-the-line motion (`FUN_0028bd90` action 2, played by
/// `FUN_0028bca0`).
const DEFEND_MOTION: usize = 0x45;

/// Takes the first free point (in `-d` order) whose path length from the
/// front is under `below` (`FUN_0028dd28` / `FUN_0028ded8`, which first
/// give up the point he had); none: the chase.
fn take_point(d: &mut Defend, at: &Atcomm, taken: &mut std::collections::HashSet<usize>, below: f32) {
    if let Some(i) = d.point.take() {
        taken.remove(&i);
    }
    d.point = (0..at.points.len()).find(|i| at.dist[*i] < below && !taken.contains(i));
    match d.point {
        Some(i) => {
            taken.insert(i);
        }
        None => d.chase = true,
    }
}

/// A defender in an alert, `-t` 0 / 1 (w03a): on joining it
/// (`FUN_00272ed0` -> `FUN_0028dd28`) he takes a point nearer the front
/// than Snake by 4 m of path, runs to it (`FUN_0028db38` sub-state 10,
/// stances 0x13 / 0x14: `FUN_0028c248`, `FUN_0028c498`, at it within
/// 100 mm) and holds it facing Snake (sub-state 9 stance 4,
/// `FUN_0028cbb0`). On the way he takes a nearer point once Snake's path
/// length from the front is within 3 m of his point's (`FUN_0028d970`) and
/// gives up for the chase once he is no nearer the front than Snake less
/// 2 m (`FUN_0028c8e8`, every 12 frames; checked each frame here); holding,
/// after 60 frames he picks again under Snake's length less 3 m once Snake
/// is 4 m of path farther out than his point, and goes back to it when
/// moved more than 250 mm off. Returns the point and whether he holds it;
/// None: the chase (the alert brain, `FUN_00283498`).
/// Holding, the first two in the defenders' order (`rank`) attack after
/// 60 frames ([`attack`]); the others hold without firing.
/// Not ported: shooting on the move (`FUN_0028a280`), the re-entry of
/// a soldier down (`FUN_002733f0` / `FUN_002732a0`) and atcomm's modes
/// (+0x2f0, from the manager's reinforcement counts).
#[allow(clippy::too_many_arguments)]
fn defend(
    g: &mut Guard,
    body: &mut Body,
    game: &Game,
    snake: Vec3,
    snake_far: f32,
    rank: usize,
    unseen: &mut u32,
    atc: &mut AtcommRun,
    taken: &mut std::collections::HashSet<usize>,
    ticks: u32,
    rng: &mut Rng,
) -> Option<Defence> {
    let at = game.atcomm.as_ref()?;
    let d = &mut g.defend;
    if d.chase || d.fixed != at.at_once {
        return None;
    }
    // `-t` 2 / 3 under a `-t` 1 atcomm: mode 2 sends them all after Snake
    // (`FUN_00272ed0` gives no point; `FUN_0028c640` the chase).
    if d.fixed && atc.mode == 2 {
        if let Some(i) = d.point.take() {
            taken.remove(&i);
        }
        d.chase = true;
        info!("defender {} leaves his point for the chase (mode 2)", g.route);
        return None;
    }
    d.frames += ticks;
    if !d.joined {
        d.joined = true;
        d.frames = 0;
        if d.fixed {
            // His own point (`FUN_00272308` / `FUN_0028ddf0`: point i for
            // the i-th of the squad).
            d.point = (d.index < at.points.len()).then_some(d.index);
            match d.point {
                Some(i) => {
                    taken.insert(i);
                }
                None => d.chase = true,
            }
        } else {
            take_point(d, at, taken, snake_far - 4000.0);
        }
        if let Some(i) = d.point {
            info!("defender {} takes point {i}", g.route);
        }
    }
    // Back in and standing where he came in (`FUN_0028d8a8`, action 1):
    // while the mode is 1; 3600 frames of it set mode 2.
    if d.stand {
        if d.frames > 3600 && atc.mode == 1 {
            atc.mode = 2;
        }
        if atc.mode != 1 {
            d.stand = false;
            if let Some(i) = d.point.take() {
                taken.remove(&i);
            }
            d.chase = true;
            info!("defender {} leaves his place for the chase", g.route);
            return None;
        }
        return Some(Defence::Stand);
    }
    // Back in: motion 0x15 (stance 0x3f, `FUN_0028c200`) before the run.
    if d.entering {
        let length = body.motions.and_then(|m| game.data.motions.get(m)).and_then(|(_, mar)| mar.motions.get(ENTER_MOTION)).map_or(30, |m| m.length as u32);
        if d.frames < length {
            return Some(Defence::Enter);
        }
        d.entering = false;
        d.frames = 0;
    }
    let i = d.point?;
    let point = at.points[i];
    if !d.holding && d.fixed {
        let off = Vec2::new(point.x - body.pos.x, point.z - body.pos.z).length();
        if off <= 100.0 || (off <= 1500.0 && g.stuck > 1.0) {
            g.stuck = 0.0;
            d.holding = true;
            d.frames = 0;
            d.spot = if off <= 100.0 { point } else { body.pos };
            info!("defender {} holds point {i}", g.route);
            return Some(Defence::Hold);
        }
        return Some(Defence::Run(point));
    }
    if !d.holding {
        let me = game.nav.as_ref().map_or(at.front.distance(body.pos), |nav| nav.path_length(at.front, body.pos, 9_000_000.0));
        if me > snake_far - 2000.0 {
            taken.remove(&i);
            d.point = None;
            d.chase = true;
            info!("defender {} leaves his point for the chase", g.route);
            return None;
        }
        if snake_far - 3000.0 < at.dist[i] {
            take_point(d, at, taken, snake_far - 3000.0);
            let j = d.point?;
            if j != i {
                info!("defender {} falls back to point {j}", g.route);
            }
            return Some(Defence::Run(at.points[j]));
        }
        // (Held too when walls keep him a little short of it for a second:
        // the port's collision is rounder than the game's. Not the game's.)
        let off = Vec2::new(point.x - body.pos.x, point.z - body.pos.z).length();
        if off <= 100.0 || (off <= 1500.0 && g.stuck > 1.0) {
            g.stuck = 0.0;
            d.holding = true;
            d.frames = 0;
            d.spot = if off <= 100.0 { point } else { body.pos };
            info!("defender {} holds point {i}", g.route);
        }
        return Some(if d.holding { Defence::Hold } else { Defence::Run(point) });
    }
    let point = d.spot;
    if d.attack.is_some() {
        return attack(g, body, game, snake, snake_far, rank, unseen, taken, ticks);
    }
    if !d.fixed && d.frames > 60 && at.dist[i] + 4000.0 < snake_far {
        take_point(d, at, taken, snake_far - 3000.0);
        let j = d.point?;
        if j != i {
            d.holding = false;
            info!("defender {} moves up to point {j}", g.route);
            return Some(Defence::Run(at.points[j]));
        }
    }
    if (point.x - body.pos.x).abs() > 250.0 || (point.z - body.pos.z).abs() > 250.0 {
        d.holding = false;
        d.frames = 0;
        return Some(Defence::Run(at.points[i]));
    }
    // `FUN_0028cbb0`: after 60 frames the first two start an attack
    // (`FUN_0028c640`): pattern 4 when Snake has been out of sight for 300
    // frames and is within 10 m of the first, else one of 0..3 at random.
    // `-t` 2 / 3 (`FUN_0028cbb0`, `FUN_0028c640`): each of them once the
    // attack gap runs out; pattern 5 while lights are on after the
    // countdown, 4 (the grenade) for the third in the order in the first
    // half of the cycle once warmed up (which starts the gap at 300), else
    // one of 0..3; then the first goes after Snake in the cycle's second
    // half (not in mode 1), and all of them in mode 2.
    let starts = if d.fixed { d.frames > 60 && atc.gap_out } else { rank < 2 && d.frames > 60 };
    if starts && at.kinds.get(i).is_some_and(|&k| (k as usize) < at.patterns.len()) {
        let pattern = if d.fixed {
            if atc.lights != 0 && atc.countdown == 0 {
                5
            } else if atc.cycle < 450 && atc.warm == 300 && rank == 2 {
                *unseen = 0;
                atc.warm = 0;
                atc.gap = 300;
                4
            } else {
                (rng.next() >> 8) as usize & 3
            }
        } else {
            let near = rank == 0 && *unseen > 300 && body.pos.distance(snake) < 10_000.0;
            if near {
                *unseen = 0;
            }
            if near { 4 } else { (rng.next() >> 8) as usize & 3 }
        };
        if d.fixed && ((atc.cycle > 449 && rank == 0 && atc.mode != 1) || atc.mode == 2) {
            taken.remove(&i);
            d.point = None;
            d.holding = false;
            d.chase = true;
            info!("defender {} leaves his point for the chase", g.route);
            return None;
        }
        d.attack = Some(PointAttack { kind: at.kinds[i] as usize, pattern, ..Default::default() });
        info!("defender {} attacks from point {i} (pattern {pattern})", g.route);
        return attack(g, body, game, snake, snake_far, rank, unseen, taken, ticks);
    }
    Some(Defence::Hold)
}

/// `FUN_0028e208` kind 3: from his hand (taken: 1400 mm up, 300 ahead) at
/// the target 10 mm above Snake's feet in an arc a sixth of the distance
/// high; with no clear line (the fire masks) at a point 1250 mm higher in
/// an arc 1000 high, else 1000 mm to either side of it. (Kind 5, for
/// sightings flagged 0x48, not ported.)
fn throw_grenade(body: &Body, snake: Vec3, game: &Game) {
    let hand = body.pos + Vec3::Y * 1400.0 + body.forward() * 300.0;
    let to = snake + Vec3::Y * 10.0;
    let clear = |p: Vec3| game.collision.as_ref().is_none_or(|c| c.ray_masked(hand, p, FIRE_WALLS, FIRE_FLOORS).is_none());
    let height = hand.distance(to) / 6.0;
    let side = Quat::from_rotation_y((to.x - hand.x).atan2(to.z - hand.z)) * Vec3::X * 1000.0;
    let (at, height) = if clear(to) {
        (to, height)
    } else if clear(to + Vec3::Y * 1250.0) {
        (to + Vec3::Y * 1250.0, 1000.0)
    } else {
        [to + side, to - side].into_iter().find(|p| clear(*p)).map_or((to, height), |p| (p, height))
    };
    crate::grenade::throw_frag(hand, at, height);
}

/// atcomm's frame under `-t` 1 (`FUN_00272820`), `ticks` times: after the
/// countdown the 900-frame cycle and the 300-frame warm-up count; fewer
/// soldiers left to come back than there are defenders (`n`) drops the
/// countdown and sets mode 1, none left mode 2; then the attack gap
/// (`FUN_002727a8`) and the countdown (+0x4e0) run down.
/// (Not ported: mode 2 when +0x4f4, a count of soldiers the decomp does
/// not show being set, reaches `n`; the countdown's call 60 frames before
/// its end with the lights on, `FUN_00213260(-1, 0, 0x4060000, ..)`.)
fn atc_frames(atc: &mut AtcommRun, at: &Atcomm, n: i32, ticks: u32) {
    if !atc.started {
        atc.started = true;
        atc.countdown = at.countdown;
        atc.lights = ((1u32 << at.lights) - 1) as u8;
    }
    atc.gap_out = false;
    for _ in 0..ticks {
        if atc.countdown == 0 {
            atc.cycle += 1;
            if atc.cycle > 900 {
                atc.cycle = 0;
            }
            atc.warm = (atc.warm + 1).min(300);
        }
        if atc.left < n {
            if atc.left != 0 {
                atc.countdown = 0;
                atc.mode = 1;
            } else {
                atc.mode = 2;
            }
        }
        if atc.gap == 0 {
            atc.gap_out = true;
            atc.gap = if atc.mode == 0 { 180 } else { 12 };
        } else {
            atc.gap -= 1;
        }
        atc.countdown = atc.countdown.saturating_sub(1);
    }
}

/// A defender of a `-t` 1 atcomm (`-t` 2 / 3) down (`FUN_002733f0`): while
/// soldiers are left to come, after 180 frames down (asleep or knocked
/// out: the dead stay down) he blinks, and at 300 he is up again at the
/// entry (`FUN_002732a0`: one fewer left), running for his point after
/// motion 0x15 (stance 0x3f), or, once fewer are left than there are
/// defenders, standing where he came in (`FUN_0028d8a8`); with none left
/// he chases at once.
/// Assumed: the game brings him back at once from 180 frames when his
/// body is off screen (`FUN_002898e8`, a view test); here always at 300.
/// He comes back with full life and no darts (`FUN_00222c08`'s reset is
/// not read).
#[allow(clippy::too_many_arguments)]
fn come_back(g: &mut Guard, body: &mut Body, at: &Atcomm, atc: &mut AtcommRun, n: i32, ticks: u32, commands: &mut Commands, me: Entity, game: &Game) {
    if !g.asleep() || g.dead() {
        g.defend.down = 0;
        return;
    }
    g.defend.down += ticks;
    let t = g.defend.down;
    if atc.left <= 0 || t <= 180 {
        return;
    }
    if t <= 300 {
        commands.entity(me).insert(if t % 2 == 1 { Visibility::Hidden } else { Visibility::Inherited });
        return;
    }
    let Some(entry) = at.entry else { return };
    atc.left -= 1;
    let index = g.defend.index;
    g.reset();
    g.state = State::Chase;
    g.meter = METER_MAX;
    g.knocked = false;
    g.roused = false;
    g.defend = Defend { fixed: true, index, ..Default::default() };
    if atc.left == 0 {
        g.defend.chase = true;
    } else if atc.left < n {
        // (atcomm, which runs before its soldiers, sets mode 1 for this
        // count in the frame before he next thinks.)
        g.defend.stand = true;
        atc.countdown = 0;
        atc.mode = 1;
    } else {
        g.defend.entering = true;
    }
    let mut p = entry;
    if let Some(y) = game.collision.as_ref().and_then(|c| c.floor_below(p.x, p.z, p.y + 1000.0)) {
        p.y = y;
    }
    body.pos = p;
    commands.entity(me).insert(Visibility::Inherited);
    info!("defender {} comes back in at {p} ({} left)", g.route, atc.left);
}

/// What a defender does this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Defence {
    /// Runs to the point.
    Run(Vec3),
    /// Holds it facing Snake (action 2, motion 0x45).
    Hold,
    /// Attacks from it (his motion set); fires a round when true.
    Attack(bool),
    /// Stands where he is facing Snake (action 1, motion 0).
    Stand,
    /// Comes back in (action 0x13: motion 0x15, once).
    Enter,
}

/// A defender's motion coming back in (`FUN_0028bd90` action 0x13).
const ENTER_MOTION: usize = 0x15;

/// The attack from the point (stance 0x1b, `FUN_0028cdf8`), `-t` 0 / 1:
/// he faces Snake and plays the pattern's steps (action, frames): frames
/// 0 = until the motion ends, 1 = the fire window's whole 150 frames,
/// more = that many frames. A shooting step (action 0x15+) fires after
/// [`ATTACK_DELAY`] frames for [`ATTACK_FIRE`], every 6 frames, while
/// Snake's path length from the front is under his point's plus his near
/// range plus 4 m (`FUN_0028ba30`) and nothing stands within 2 m short of
/// Snake on the line from his gun (`FUN_0028a448`; else 12 frames' wait).
/// Every 12 frames he gives up for the chase (`FUN_0028c8e8`) once he is
/// no nearer the front than Snake less 2 m, or (the first in the order)
/// once Snake has been unseen 300 frames and is 10 m off. The pattern
/// over (action 0x1e), he holds again if within 500 mm of the point, else
/// runs back to it.
/// Pattern 4 throws a grenade ([`throw_grenade`]).
/// Not ported: the aim's spread (+0x14, which nothing in w03a seems to
/// set), the jump past step 0 under flag 4, the grenade in his hand from
/// frame 0x21, mirrored motions.
#[allow(clippy::too_many_arguments)]
fn attack(
    g: &mut Guard,
    body: &mut Body,
    game: &Game,
    snake: Vec3,
    snake_far: f32,
    rank: usize,
    unseen: &mut u32,
    taken: &mut std::collections::HashSet<usize>,
    ticks: u32,
) -> Option<Defence> {
    let at = game.atcomm.as_ref()?;
    let i = g.defend.point?;
    let mut a = g.defend.attack?;
    let steps = &at.patterns.get(a.kind)?[a.pattern];
    let dist = body.pos.distance(snake);
    let length_of = |motion: usize| body.motions.and_then(|m| game.data.motions.get(m)).and_then(|(_, mar)| mar.motions.get(motion)).map_or(30, |m| m.length as u32);
    let mut fire = false;
    for _ in 0..ticks.max(1) {
        if !g.defend.fixed && a.frames.is_multiple_of(12) {
            let me = game.nav.as_ref().map_or(at.front.distance(body.pos), |nav| nav.path_length(at.front, body.pos, 5_000_000.0));
            if me > snake_far - 2000.0 || (rank == 0 && *unseen > 300 && dist >= 10_000.0) {
                taken.remove(&i);
                g.defend.point = None;
                g.defend.attack = None;
                g.defend.holding = false;
                g.defend.chase = true;
                info!("defender {} leaves his point for the chase", g.route);
                return None;
            }
        }
        let (action, length) = steps.get(a.step).copied().unwrap_or((0x1e, 0));
        if action >= 0x15
            && a.count > ATTACK_DELAY
            && a.count <= ATTACK_DELAY + ATTACK_FIRE
            && a.frames.is_multiple_of(6)
            && a.cooldown == 0
            && (g.defend.fixed || snake_far < at.dist[i] + g.vision.distance + 4000.0)
        {
            let muzzle = body.pos + Vec3::Y * MUZZLE;
            let target = snake + Vec3::Y * 1000.0;
            let len = muzzle.distance(target);
            let ray = game.collision.as_ref().and_then(|c| c.ray_masked(muzzle, target, FIRE_WALLS, FIRE_FLOORS));
            let clear = ray.is_none_or(|t| t * len >= len - 2000.0);
            if clear {
                fire = true;
                if !a.fired {
                    a.fired = true;
                    info!("defender {} fires from point {i}", g.route);
                }
            } else {
                a.cooldown = 12;
            }
        }
        a.cooldown = a.cooldown.saturating_sub(1);
        // Pattern 4's grenade (action 0x12, `FUN_0028eab0`): thrown on
        // frame 0x60 of motion 0x4e at Snake (0x1ece00, +0xf80).
        if action == 0x12 && a.count == 0x60 {
            throw_grenade(body, snake, game);
            info!("defender {} throws a grenade", g.route);
        }
        // The motion's own travel: 0x41 / 0x40 step about 540 mm to the
        // side and back (out from cover and in again; 0x107 / 0x108 45 mm),
        // which the game's root track moves him by.
        if let Some(m) = defend_action(action).and_then(|(m, _)| body.motions.and_then(|i| game.data.motions.get(i)).and_then(|(_, mar)| mar.motions.get(m)))
            && m.length > 0
            && let (Some(p0), Some(p1)) = {
                let f0 = (a.count % m.length) as f32;
                (m.root_position(f0), m.root_position((f0 + 1.0).min(m.length as f32 - 1.0)))
            }
        {
            let d = Quat::from_rotation_y(body.yaw) * Vec3::new(p1[0] - p0[0], 0.0, p1[2] - p0[2]);
            let mut p = body.pos + d;
            if let Some(col) = &game.collision {
                p = col.push_out(p, RADIUS, HEIGHT, STEP_UP);
                p.y = col.floor_below(p.x, p.z, p.y + STEP_UP).unwrap_or(body.pos.y);
            }
            body.pos = p;
        }
        a.frames += 1;
        // (Steps of 0 frames end with their once-played motion, the game's
        // +0xf58; counted here by the motion's length.)
        a.count += 1;
        let ended = match length {
            0 => defend_action(action).is_none_or(|(m, _)| a.count >= length_of(m)),
            1 => a.count > ATTACK_DELAY + ATTACK_FIRE + ATTACK_AFTER,
            n => a.count >= u32::from(n),
        };
        if ended {
            a.step += 1;
            a.count = 0;
            a.cooldown = 0;
            a.fired = false;
            if steps.get(a.step).is_none_or(|s| s.0 == 0x1e) {
                g.defend.attack = None;
                g.defend.frames = 0;
                let spot = g.defend.spot;
                if (spot.x - body.pos.x).abs() >= 500.0 || (spot.z - body.pos.z).abs() >= 500.0 {
                    g.defend.holding = false;
                    return Some(Defence::Run(at.points[i]));
                }
                return Some(Defence::Hold);
            }
        }
    }
    let (action, _) = steps.get(a.step).copied().unwrap_or((2, 0));
    match defend_action(action) {
        Some((m, true)) => body.play_once(m),
        Some((m, false)) => body.play(m),
        None => body.play(DEFEND_MOTION),
    }
    g.defend.attack = Some(a);
    Some(Defence::Attack(fire))
}

#[allow(clippy::too_many_arguments)]
fn alert_brain(
    g: &mut Guard,
    body: &mut Body,
    snake: Vec3,
    (snake_dead, snake_open): (bool, bool),
    goal: Option<Defence>,
    dt: f32,
    ticks: u32,
    game: &Game,
    rng: &mut Rng,
) -> bool {
    if g.level == 3 {
        g.meter = METER_MAX;
    }
    // Tracking flag times out 60 frames after the last sighting.
    if let Some(t) = g.tracking.as_mut() {
        *t += ticks;
        if *t > 60 {
            g.tracking = None;
        }
    }
    // Out of sight and no longer tracked: to his post if he has one
    // (`FUN_00228710` with +0x2c0 set), else after Snake's last place.
    // (A post he cannot get to is given up.)
    if g.stuck > 2.0 && g.level != 3 && g.tracking.is_none() && g.post.is_some() {
        g.post = None;
        g.stuck = 0.0;
    }
    let target = match (g.level, g.tracking, g.post) {
        (3, _, _) => snake,
        (_, None, Some(post)) => post,
        _ => g.last_seen.unwrap_or(snake),
    };
    let frames = dt * 60.0;

    // In sight and in range: stand and shoot.
    let a = &mut g.attack;
    if a.reload > 0.0 {
        a.reload -= frames;
        if g.anim.knocked.is_some() {
            body.play_once(RELOAD_MOTION);
        } else {
            body.play(g.anim.idle);
        }
        face(body, target, dt);
        return false;
    }
    // Close quarters: the rifle butt (alert action 5, or 9 with a wounded
    // arm: `FUN_002281e8`).
    let (motion, joint) = if g.wounds & 1 != 0 { (WOUNDED_STRIKE_MOTION, WOUNDED_STRIKE_JOINT) } else { (STRIKE_MOTION, STRIKE_JOINT) };
    if let Some((t, landed)) = a.strike {
        let t = t + frames;
        body.play_once(motion);
        face(body, snake, dt);
        let mut landed = landed;
        if !landed && STRIKE_FRAMES.contains(&t) && !snake_dead {
            if strike_hits(game, body, joint, snake) {
                info!("guard {} strikes in the alert (motion {motion:#x})", g.route);
                landed = true;
                land_strike(body);
            }
        }
        let length = body.motions.and_then(|m| game.data.motions.get(m)).and_then(|(_, mar)| mar.motions.get(motion)).map_or(40.0, |m| m.length as f32);
        a.strike = if t >= length { None } else { Some((t, landed)) };
        return false;
    }
    // A defender: on his way to his point he only runs; at it he holds
    // the line facing Snake, or attacks from it ([`attack`], which plays
    // his motions).
    match goal {
        Some(Defence::Run(point)) => {
            a.firing = false;
            a.aim = 0.0;
            if g.path.last() != Some(&point) {
                plan(g, body.pos, point, game);
            }
            if follow(g, body, RUN_SPEED, 100.0, dt, game) {
                body.play(DEFEND_MOTION);
            } else {
                body.play(g.anim.run);
            }
            return false;
        }
        Some(Defence::Hold) => {
            g.path.clear();
            a.firing = false;
            a.aim = 0.0;
            body.play(DEFEND_MOTION);
            face(body, snake, dt);
            return false;
        }
        Some(Defence::Attack(fire)) => {
            g.path.clear();
            face(body, snake, dt);
            return fire && !snake_dead;
        }
        Some(Defence::Stand) => {
            g.path.clear();
            a.firing = false;
            a.aim = 0.0;
            body.play(0);
            face(body, snake, dt);
            return false;
        }
        Some(Defence::Enter) => {
            g.path.clear();
            a.firing = false;
            a.aim = 0.0;
            body.play_once(ENTER_MOTION);
            return false;
        }
        None => {}
    }
    // Not at a Snake prone or down (`DAT_001ecd40 & 0x620`, tested by
    // every way into sub-state 6: `FUN_002284b8`, `FUN_00228710`,
    // `FUN_00228e40`, `FUN_002290b0`): he is shot at instead.
    if g.level == 3 && body.pos.distance(snake) < STRIKE_RANGE && !snake_dead && snake_open && g.anim.knocked.is_some() {
        g.path.clear();
        a.strike = Some((0.0, false));
        a.firing = false;
        a.aim = 0.0;
        return false;
    }
    if (g.level == 3 || body.pos.distance(snake) < POINT_BLANK) && !snake_dead {
        g.path.clear();
        body.play(g.anim.aim);
        face(body, snake, dt);
        if !a.firing {
            a.aim += frames;
            if a.aim >= AIM_FRAMES {
                a.firing = true;
                a.cooldown = 0.0;
            }
            return false;
        }
        a.cooldown -= frames;
        if a.cooldown > 0.0 {
            return false;
        }
        a.cooldown = SHOT_FRAMES;
        a.shots += 1;
        if a.shots >= MAGAZINE {
            a.shots = 0;
            info!("guard {} reloads", g.route);
            a.reload = if g.anim.knocked.is_some() {
                body.motions.and_then(|m| game.data.motions.get(m)).and_then(|(_, mar)| mar.motions.get(RELOAD_MOTION)).map_or(RELOAD_FRAMES, |m| m.length as f32)
            } else {
                RELOAD_FRAMES
            };
            a.firing = false;
            a.aim = 0.0;
        } else if rng.next().is_multiple_of(3) {
            // Burst over: aim again.
            a.firing = false;
            a.aim = 0.0;
        }
        return true;
    }
    a.firing = false;
    a.aim = 0.0;

    g.repath = g.repath.saturating_sub(ticks);
    if g.path.is_empty() || g.repath == 0 {
        plan(g, body.pos, target, game);
    }
    let reach = if g.level == 3 { 2500.0 } else { 600.0 };
    if follow(g, body, RUN_SPEED, reach, dt, game) {
        body.play(g.anim.idle);
        face(body, target, dt);
    } else {
        body.play(g.anim.run);
    }
    false
}

/// A noise or a footprint: a soldier at peace goes to look
/// (`FUN_00227520`); one searching in Evasion takes his search there
/// (`FUN_0022c168` handles the same stimuli).
fn stimulus(g: &mut Guard, body: &Body, at: Vec3, game: &Game) {
    if let State::Search { facing, .. } = g.state {
        plan(g, body.pos, at, game);
        g.state = State::Search { wait: 0, facing };
    } else {
        g.state = State::Investigate { target: at, sub: 0, timer: 0, asks: false };
    }
}

/// `FUN_0022C670` stand-in: walk to the last known position, then visit
/// nav nodes nearby, pausing to look around.
fn evasion_brain(g: &mut Guard, body: &mut Body, phase: &Phase, dt: f32, ticks: u32, game: &Game) {
    let State::Search { wait, facing } = g.state else {
        g.state = State::Search { wait: 0, facing: body.yaw };
        return;
    };
    if wait > 0 {
        // Looking around where he stopped (`FUN_0022adc0` sub-state 6,
        // `FUN_0022ac58`): the head (+0xf66, see [`Guard::turn_head`])
        // 0x200 (45 degrees) to one side for frames 1..59, to the other
        // for 61..119, of 180; the body keeps its facing.
        body.play(g.anim.idle);
        let seen = LOOK_FRAMES.saturating_sub(wait);
        g.head_to = match seen {
            1..=59 => 512.0,
            61..=119 => -512.0,
            _ => 0.0,
        };
        body.yaw = turn_toward(body.yaw, facing, TURN_RATE * dt);
        let left = wait.saturating_sub(ticks);
        g.state = State::Search { wait: left, facing };
        if left == 0 {
            let around = phase.last_seen.unwrap_or(body.pos);
            if let Some(nav) = &game.nav {
                let near = nav.nodes_near(around, 8000.0);
                if !near.is_empty() {
                    let pick = near[(body.pos.x.abs() as usize + wait as usize * 7) % near.len()];
                    plan(g, body.pos, nav.node_pos(pick), game);
                }
            }
        }
        return;
    }
    body.play(g.anim.walk);
    if follow(g, body, g.anim.walk_speed, 400.0, dt, game) {
        g.state = State::Search { wait: LOOK_FRAMES, facing: body.yaw };
    }
}

/// Enemy manager per frame: `FUN_00220430` (meter) and `FUN_00220818`
/// (phase changes).
fn manager(p: &mut Phase, highest_guard_meter: i32) {
    p.meter = (p.meter - 16).max(0).max(highest_guard_meter);
    match p.kind {
        PhaseKind::Infiltration | PhaseKind::Caution => {
            // A caution timer set from outside (a radio report) turns
            // Infiltration into Caution (`FUN_00220818`, phase 0).
            if p.kind == PhaseKind::Infiltration && p.caution >= 1 {
                p.set(PhaseKind::Caution);
            }
            if p.kind == PhaseKind::Caution {
                p.caution -= 1;
                if p.caution < 1 {
                    p.set(PhaseKind::Infiltration);
                }
            }
            if p.meter >= 0x400 {
                p.meter = 0x400;
                p.set(PhaseKind::Alert);
            }
        }
        PhaseKind::Alert => {
            if p.meter < 1 {
                p.evasion = 0x4B0;
                p.set(PhaseKind::Evasion);
            }
        }
        PhaseKind::Evasion => {
            if p.evasion < 1 {
                p.caution = p.caution_time;
                p.set(PhaseKind::Caution);
            }
            if p.meter >= 0x400 {
                p.meter = 0x400;
                p.set(PhaseKind::Alert);
            }
            p.evasion -= 1;
        }
    }
}

/// `--trace`: log guard positions and states once a second.
#[derive(Resource, Default)]
pub struct Trace(pub f32);

pub fn trace(
    time: Res<Time>,
    mut t: ResMut<Trace>,
    phase: Res<Phase>,
    guards: Query<(&Guard, &Body)>,
    snake: Query<(&Body, &Player)>,
    bullets: Query<&bullet::Bullet>,
    game: Res<Game>,
    mut routes_shown: Local<bool>,
) {
    if !*routes_shown && !guards.is_empty() {
        *routes_shown = true;
        if std::env::var_os("MGS2_ALL_ROUTES").is_some()
            && let Some(c) = game.data.collision.as_ref()
        {
            for (i, r) in c.routes.iter().enumerate() {
                let pts: Vec<String> = r.points.iter().take(3).map(|w| format!("({:.0},{:.0},{:.0})", w.position[0], w.position[1], w.position[2])).collect();
                eprintln!("all route {i}: {}", pts.join(" "));
            }
        }
        for (g, _) in &guards {
            if let Some(r) = game.data.collision.as_ref().and_then(|c| c.routes.get(g.route)) {
                let pts: Vec<String> = r.points.iter().map(|w| format!("({:.0},{:.0},{:.0} w{})", w.position[0], w.position[1], w.position[2], w.wait)).collect();
                eprintln!("route {} (hearing {}): {}", g.route, g.hearing, pts.join(" "));
            }
        }
    }
    t.0 += time.delta_secs();
    if t.0 < 1.0 {
        return;
    }
    t.0 = 0.0;
    let (s, life) = snake.single().map(|(b, p)| (b.pos, p.life)).unwrap_or_default();
    eprint!(
        "t={:5.1} snake=({:.0},{:.0},{:.0}) life={} bullets={} {:?} meter={} ev={} ca={} |",
        time.elapsed_secs(),
        s.x,
        s.y,
        s.z,
        life,
        bullets.iter().count(),
        phase.kind,
        phase.meter,
        phase.evasion,
        phase.caution
    );
    for (g, b) in &guards {
        let st = match &g.state {
            State::Investigate { sub, timer, .. } => format!("Investigate{sub}/{timer}"),
            State::Discovery { timer } => format!("Discovery/{timer}"),
            State::Wait(w) => format!("Wait{w:.1}"),
            State::Comrade { sub, timer, .. } => format!("Comrade{sub:#x}/{timer}"),
            State::Call { sub, timer, .. } => format!("Call{sub:#x}/{timer}"),
            State::CheckIn { sub, timer } => format!("CheckIn{sub:#x}/{timer}"),
            s => format!("{s:?}"),
        };
        eprint!(" r{} ({:.0},{:.0},{:.0}) {} L{} m{} |", g.route, b.pos.x, b.pos.y, b.pos.z, st, g.level, g.meter);
    }
    eprintln!();
}

/// Debug view: vision cones (near and far range) and state markers.
pub fn draw(game: Res<Game>, guards: Query<(&Guard, &Body)>, mut gizmos: Gizmos) {
    use mgs2_bevy::scene::SCALE;
    for (g, b) in &guards {
        let eye = (b.pos + Vec3::Y * EYE) * SCALE;
        let colour = match (&g.state, g.level) {
            (State::Chase | State::Discovery { .. } | State::Call { .. }, _) | (_, 3) => Color::srgb(1.0, 0.1, 0.1),
            (State::Investigate { .. } | State::Search { .. }, _) | (_, 2) => Color::srgb(1.0, 0.8, 0.1),
            _ => Color::srgba(0.3, 0.8, 1.0, 0.7),
        };
        if game.show_vision {
            let half = g.vision.half_angle as f32 * ANGLE;
            for (range, alpha) in [(g.vision.distance, 1.0), (g.vision.distance * 1.5, 0.4)] {
                for side in [-1.0, 1.0] {
                    let yaw = b.yaw + side * half;
                    let end = eye + Vec3::new(yaw.sin(), 0.0, yaw.cos()) * range * SCALE;
                    gizmos.line(eye, end, colour.with_alpha(alpha));
                }
            }
        }
        if game.show_vision && !matches!(g.state, State::Patrol | State::Wait(_) | State::Return) {
            gizmos.sphere(Isometry3d::from_translation(eye + Vec3::Y * 0.6), 0.12, colour);
        }
        if game.show_vision {
            for pair in g.path.windows(2) {
                gizmos.line(pair[0] * SCALE + Vec3::Y * 0.1, pair[1] * SCALE + Vec3::Y * 0.1, colour);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gcl::Val;

    #[test]
    fn drops_follow_their_list() {
        let s = Spawn {
            kind: GUARD_ACTOR,
            args: vec![Val::Int(0x26539FC)],
            options: vec![(b'r', vec![Val::Int(3)]), (b'q', vec![Val::Proc(0x10), Val::Int(100), Val::Proc(0x11), Val::Int(0)])],
        };
        let (mut g, _) = from_spawn(&s, 6000.0).unwrap();
        assert_eq!(g.drops[0], vec![(0x10, 100), (0x11, 0)]);
        assert!(g.drops[1].is_empty());
        assert_eq!(g.name, 0x26539FC);
        let mut rng = Rng(12345);
        DROPS.lock().unwrap().clear();
        // A sure drop runs and moves on; a 0% one never does.
        try_drop(&mut g, 0, 1.0, Vec3::ZERO, &mut rng);
        try_drop(&mut g, 0, 1.0, Vec3::ZERO, &mut rng);
        try_drop(&mut g, 0, 1.0, Vec3::ZERO, &mut rng);
        assert_eq!(g.dropped[0], 1);
        let q = DROPS.lock().unwrap().clone();
        assert_eq!(q.len(), 1);
        assert_eq!((q[0].0, q[0].1[7]), (0x10, 0x26539FC));
    }

    #[test]
    fn loudest_noise_wins() {
        NOISE.lock().unwrap().take();
        noise(5, Vec3::X);
        noise(6, Vec3::Y);
        noise(5, Vec3::Z);
        noise(0, Vec3::ONE);
        assert_eq!(NOISE.lock().unwrap().take(), Some((6, Vec3::Y)));
    }

    #[test]
    fn darts_follow_the_game_drain() {
        // Normal: a body dart leaves 2000, drained 4 a frame.
        let mut g = patrol(0, 6000.0, Anim::SOLDIER);
        g.dart(Zone::Body);
        g.drowse(500, 30);
        assert!(!g.asleep());
        g.drowse(1, 30);
        assert!(g.asleep());
        // A head dart leaves 0; the next frame's drain puts him under.
        let mut g = patrol(0, 6000.0, Anim::SOLDIER);
        g.dart(Zone::Head);
        g.drowse(1, 30);
        assert!(g.asleep());
        // Very Easy: any dart within a frame.
        let mut g = patrol(0, 6000.0, Anim::SOLDIER);
        g.dart(Zone::Limb(2));
        g.drowse(1, 10);
        assert!(g.asleep());
    }

    #[test]
    fn three_body_hits_kill() {
        let mut g = patrol(0, 6000.0, Anim::SOLDIER);
        assert!(!g.shot(Zone::Body));
        assert!(!g.shot(Zone::Body));
        assert!(g.shot(Zone::Body));
        assert!(g.dead());
    }

    #[test]
    fn head_kills_and_limb_pairs_kill() {
        let mut g = patrol(0, 6000.0, Anim::SOLDIER);
        assert!(g.shot(Zone::Head));
        let mut g = patrol(0, 6000.0, Anim::SOLDIER);
        assert!(!g.shot(Zone::Limb(0)));
        assert!(!g.shot(Zone::Limb(2)));
        assert!(g.shot(Zone::Limb(1)));
        g.reset();
        assert_eq!((g.life, g.wounds), (DEFAULT_LIFE, 0));
    }
}
