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

use std::f32::consts::TAU;

use bevy::prelude::*;
use mgs2_formats::hzx::Waypoint;

use crate::Game;
use crate::body::{Body, turn_toward};
use crate::bullet::{self, Rng};
use crate::gcl::Spawn;
use crate::player::{HEIGHT, Player, RADIUS, STEP_UP};

pub const GUARD_ACTOR: u32 = 0xF7F777;
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
}

/// A soldier's report on the radio (`FUN_0021cdb8(0x1c00000b)`).
pub const LINE_REPORT: u32 = 0x1C00_000B;
/// "It was nothing" (0xc000012, a comrade up again in Infiltration).
pub const LINE_ALL_CLEAR: u32 = 0x0C00_0012;
/// Lines per voice in the `-v` table.
const LINES_PER_VOICE: u32 = 0x38;
/// The radio click (`FUN_00114b30(0x20, 0x3f, 99)`).
const RADIO_CLICK: u32 = 99;

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
        let replies = words(b'y').into_iter().take_while(|&w| w >= 0).map(|w| w as u32).collect();
        Self { lines, replies }
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
    fn speak(&mut self, line: u32, g: &Guard) {
        let Some((id, _)) = self.radio.line(line, g) else { return };
        if line & 0x4_0000 == 0 {
            crate::sfx::play(RADIO_CLICK, None);
        }
        self.say.push(id);
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
        }
    }
}

impl Phase {
    pub fn from_manager(s: Option<&Spawn>, script: &mgs2_formats::gcx::Gcx) -> Self {
        let mut p = Self::default();
        if let Some(s) = s {
            p.caution_time = s.opt_int(b'h', 0).unwrap_or(0xE10);
            p.radio = Radio::from_script(s, script);
        }
        p
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
    Investigate { target: Vec3, sub: u8, timer: u32 },
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
    /// Normal 11: walked into Snake's box and looks it over
    /// (`FUN_00242448`), frames so far.
    BoxCheck { timer: u32 },
}

/// The motions a guard plays (indices into his motion archive).
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    pub idle: usize,
    pub walk: usize,
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
}

impl Anim {
    /// Gurlukovich soldiers, `gbs.mar`.
    pub const SOLDIER: Anim = Anim { idle: motion::IDLE, walk: motion::WALK, run: motion::RUN, aim: motion::AIM, hands_up: 8, fall: sleep::FALL, lie: sleep::LIE, exclaim: motion::EXCLAIM, look: Some(motion::LOOK), knocked: Some((hurt::FALL, hurt::GET_UP)), get_up: Some(((sleep::GET_UP, 95.0), (sleep::ROUSED, 175.0))), check: Some(Check { shake: 76, kick: 47, radio_out: 35, radio_talk: 27, radio_away: 28 }) };
    /// US Marines, `ushold.mar`: 39 and 40 cover 1332 and 4057 mm/s, the
    /// soldiers' walk and run; 46 hands up, 151 falls forward, 152 lies.
    /// No rifle-raised motion was found, so they aim from the idle pose.
    pub const MARINE: Anim = Anim { idle: 0, walk: 39, run: 40, aim: 0, hands_up: 46, fall: 151, lie: 152, exclaim: 0, look: None, knocked: None, get_up: None, check: None };
}
/// A held-up guard turns on Snake after this long without a gun on him.
const HOLDUP_LOOSE: f32 = 120.0;
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
    pub anim: Anim,
}

/// A shot Snake made for the guards to hear: where, and how far it carries
/// (mm): the USP.
#[derive(Resource, Default)]
pub struct Gunshot(pub Option<(Vec3, f32)>);

/// How far the USP is heard (mm; not from the game).
pub const GUNSHOT_RANGE: f32 = 10_000.0;

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
/// What a strike takes from Snake (the hit's damage is not read yet;
/// assumed) and the reach of its hit box (assumed).
const STRIKE_DAMAGE: i32 = 15;
const STRIKE_REACH: f32 = 1100.0;

/// Strikes that landed this frame (damage), taken by the player.
pub static STRIKES: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

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
        wounds: 0,
        stamina,
        max_stamina: stamina,
        resist: RESIST,
        darts: [0; 3],
        roused: false,
        knocked: false,
        checked: false,
        radio_in_hand: false,
        stuck: 0.0,
        post: post_at,
        tagged: false,
        drops: [drop_list(s, b'q'), drop_list(s, b'a')],
        dropped: [0, 0],
        name: s.args.first().and_then(crate::gcl::Val::int).unwrap_or(0),
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
        stuck: 0.0,
        post: None,
        tagged: false,
        drops: [Vec::new(), Vec::new()],
        dropped: [0, 0],
        name: 0,
        anim,
    }
}

impl Guard {
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
    let head = to_units(body.yaw);
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
        let step = (speed * dt).min(flat.length());
        let mut p = body.pos + body.forward() * step;
        if let Some(col) = &game.collision {
            p = col.push_out(p, RADIUS, HEIGHT, STEP_UP);
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
    snake: Query<(&Body, &Player), Without<Guard>>,
    mut guards: Query<(Entity, &mut Guard, &mut Body)>,
    mut clock: Local<f32>,
    mut down: Local<std::collections::HashSet<Entity>>,
) {
    // The game's logic runs once per 60 Hz frame; counters advance by the
    // whole frames elapsed (movement uses the real time step).
    *clock += time.delta_secs().min(0.05) * 60.0;
    let ticks = clock.floor() as u32;
    *clock -= ticks as f32;
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
    let changed = std::mem::take(&mut phase.changed);
    let mut highest = 0i32;

    let shot = gunshot.0.take();
    let difficulty = game.banks.b16(crate::gcl::DIFFICULTY);
    // MGS2_DOWN=route:s|k|d@seconds,..: that soldier falls asleep, is
    // knocked out or dies then (testing).
    let downs: Vec<(usize, char)> = std::env::var("MGS2_DOWN")
        .ok()
        .map(|v| {
            v.split(',')
                .filter_map(|e| {
                    let (who, at) = e.split_once('@')?;
                    let (route, kind) = who.split_once(':')?;
                    let at: f32 = at.parse().ok()?;
                    let now = time.elapsed_secs();
                    (now >= at && now - dt < at).then_some((route.parse().ok()?, kind.chars().next()?))
                })
                .collect()
        })
        .unwrap_or_default();
    for (me, mut g, mut body) in &mut guards {
        if let Some(&(_, kind)) = downs.iter().find(|d| d.0 == g.route) {
            info!("test: guard {} down ({kind})", g.route);
            match kind {
                'k' => g.knock_out(),
                'd' => g.kill(),
                _ => g.fall_asleep(),
            }
        }
        if !matches!(g.state, State::Comrade { .. }) {
            g.radio_in_hand = false;
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
            && to_guard.normalize_or_zero().angle_to(Vec2::new(snake_fwd.x, snake_fwd.z)).abs() < 0.5;
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
                State::Discovery { timer: 0 }
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
        g.level = perceive(&g, &body, snake_pos, snake_height, &game, sight);
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
            g.state = State::Investigate { target: snake_pos, sub: 0, timer: 0 };
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
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. } | State::Search { .. })
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
        // A noise within earshot draws a look (ranges assumed).
        if let Some((at, range)) = shot
            && g.level < 2
            && body.pos.distance(at) < range
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. })
        {
            // An unsuppressed shot (noise 10) is heard as an attack: stimulus
            // 0x800, straight to discovery (`FUN_00227520` ->
            // `FUN_00238f68`).
            if range >= GUNSHOT_RANGE {
                g.last_seen = Some(at);
                g.state = State::Discovery { timer: 0 };
            } else {
                g.state = State::Investigate { target: at, sub: 0, timer: 0 };
            }
        }
        // Footprints in clear view within his near range: he goes to the
        // newest he sees and leaves the older ones (`FUN_00230948`,
        // `FUN_00240858`); the next ones then lead him on.
        if g.level < 2
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. } | State::Search { .. })
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
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. } | State::Search { .. })
        {
            info!("guard {} heard a noise at {at:?}", g.route);
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

        match phase.kind {
            PhaseKind::Alert => {
                if alert_brain(&mut g, &mut body, snake_pos, snake_dead, dt, ticks, &game, &mut rng) {
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
                    normal_brain(&mut g, &mut body, snake_pos, player.boxed, dt, ticks, &game);
                } else {
                    evasion_brain(&mut g, &mut body, &phase, dt, ticks, &game);
                }
            }
            _ => normal_brain(&mut g, &mut body, snake_pos, player.boxed, dt, ticks, &game),
        }
        highest = highest.max(i32::from(g.meter));
    }

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
            body.play_once(c.radio_out);
            g.radio_in_hand = t >= 28;
            if t >= motion_length(game, body, c.radio_out, 99) {
                next = Some((0xc, 0));
            }
        }
        0xc => {
            let Some(c) = check else { return None };
            // `FUN_00231d88`: the report on the first frame; at its end
            // the radio goes away, the caution timer runs and HQ answers.
            if timer == 0 && t > 0 {
                phase.speak(LINE_REPORT, g);
            }
            let frames = phase.line_frames(LINE_REPORT, g, REPORT_FRAMES);
            if t < frames {
                body.play(c.radio_talk);
            } else {
                body.play_once(c.radio_away);
                g.radio_in_hand = t < frames + 62;
                if reached(frames) {
                    out = Some(Seen::Report);
                    phase.reply();
                }
                if t >= frames + motion_length(game, body, c.radio_away, 129) {
                    next = None;
                }
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
fn normal_brain(g: &mut Guard, body: &mut Body, snake: Vec3, boxed: bool, dt: f32, ticks: u32, game: &Game) {
    // Transitions out of patrol/wait/return, in the game's priority order
    // (later checks override earlier ones).
    if matches!(g.state, State::Patrol | State::Wait(_) | State::Return) {
        if g.meter > 0 {
            let target = g.last_seen.unwrap_or(snake);
            g.state = State::Investigate { target, sub: 0, timer: 0 };
        }
        if g.meter >= 2 || g.level == 3 {
            g.state = State::Discovery { timer: 0 };
        }
    }
    // Investigation escalates (FUN_00227430).
    if matches!(g.state, State::Investigate { .. }) && g.meter > 8 {
        g.state = State::Discovery { timer: 0 };
    }

    match g.state.clone() {
        State::Patrol => {
            let Some(wp) = waypoint(game, g.route, g.node) else {
                body.play(g.anim.idle);
                return;
            };
            body.play(g.anim.walk);
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
            if follow(g, body, WALK_SPEED, 150.0, dt, game) {
                // A route of one point is a post: he stands there.
                let posts = game.data.collision.as_ref().and_then(|c| c.routes.get(g.route)).is_some_and(|r| r.points.len() <= 1);
                g.state = match wp.wait {
                    _ if posts => State::Wait(f32::INFINITY),
                    0 => {
                        g.node += 1;
                        State::Patrol
                    }
                    w if w < 0 => State::Wait(f32::INFINITY),
                    w => State::Wait(f32::from(w) / 60.0),
                };
            }
        }
        State::Wait(left) => {
            body.play(g.anim.idle);
            if let Some(wp) = waypoint(game, g.route, g.node) {
                let f = match wp.look_at {
                    Some(l) => (l[0] - body.pos.x).atan2(l[2] - body.pos.z),
                    None => f32::from(wp.direction) * ANGLE,
                };
                body.yaw = turn_toward(body.yaw, f, TURN_RATE * dt);
            }
            let left = left - dt;
            g.state = if left <= 0.0 {
                g.node += 1;
                State::Patrol
            } else {
                State::Wait(left)
            };
        }
        State::Investigate { target, sub, timer } => {
            // Sub-states: 0 glance toward the target, 1 walk there,
            // 2 look around (sub-state 10: up to 300 frames), then return.
            let before = timer;
            let timer = timer + ticks;
            g.state = match sub {
                0 => {
                    body.play(g.anim.idle);
                    face(body, target, dt);
                    if timer > 30 {
                        plan(g, body.pos, target, game);
                        State::Investigate { target, sub: 1, timer: 0 }
                    } else {
                        State::Investigate { target, sub, timer }
                    }
                }
                1 => {
                    body.play(g.anim.walk);
                    if follow(g, body, WALK_SPEED, 600.0, dt, game) || timer > 600 {
                        State::Investigate { target, sub: 2, timer: 0 }
                    } else {
                        State::Investigate { target, sub, timer }
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
                        State::Investigate { target, sub, timer }
                    }
                }
            };
        }
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
            // "!": face Snake while the reaction plays, then the meter goes
            // to 0x400, which raises the alert through the manager.
            body.play_once(g.anim.exclaim);
            face(body, snake, dt);
            let length = body
                .motions
                .and_then(|m| game.data.motions.get(m))
                .and_then(|(_, mar)| mar.motions.get(g.anim.exclaim))
                .map_or(EXCLAIM_FRAMES, |m| m.length.max(1));
            let before = timer;
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
                g.meter = METER_MAX;
            }
            g.state = State::Discovery { timer };
        }
        State::Return => {
            body.play(g.anim.walk);
            if follow(g, body, WALK_SPEED, 300.0, dt, game) {
                g.path.clear();
                g.state = State::Patrol;
            }
        }
        State::Chase | State::Search { .. } => g.state = State::Return,
        State::Asleep { .. } | State::Dead { .. } | State::Comrade { .. } | State::HeldUp { .. } | State::Hurt { .. } => {}
    }
}

/// `FUN_00229D30`: chase, and shoot when Snake is in sight. Returns true
/// when a shot is fired this frame.
#[allow(clippy::too_many_arguments)]
fn alert_brain(
    g: &mut Guard,
    body: &mut Body,
    snake: Vec3,
    snake_dead: bool,
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
    // Close quarters: the rifle butt.
    if let Some((t, landed)) = a.strike {
        let t = t + frames;
        body.play_once(STRIKE_MOTION);
        face(body, snake, dt);
        let mut landed = landed;
        if !landed && (2.0..=6.0).contains(&t) && !snake_dead {
            let to = snake - body.pos;
            if to.length() < STRIKE_REACH && to.normalize_or_zero().dot(body.forward()) > 0.5 {
                landed = true;
                STRIKES.fetch_add(STRIKE_DAMAGE, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let length = body.motions.and_then(|m| game.data.motions.get(m)).and_then(|(_, mar)| mar.motions.get(STRIKE_MOTION)).map_or(40.0, |m| m.length as f32);
        a.strike = if t >= length { None } else { Some((t, landed)) };
        return false;
    }
    if g.level == 3 && body.pos.distance(snake) < STRIKE_RANGE && !snake_dead && g.anim.knocked.is_some() {
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
        g.state = State::Investigate { target: at, sub: 0, timer: 0 };
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
        // Looking around where he stopped (`FUN_0022adc0` sub-state 6):
        // 0x200 (45 degrees) to one side for frames 1..59, to the other
        // for 61..119, of 180.
        body.play(g.anim.idle);
        let seen = LOOK_FRAMES.saturating_sub(wait);
        let aim = match seen {
            1..=59 => facing + 0x200 as f32 * ANGLE,
            61..=119 => facing - 0x200 as f32 * ANGLE,
            _ => facing,
        };
        body.yaw = turn_toward(body.yaw, aim, TURN_RATE * dt);
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
    if follow(g, body, WALK_SPEED, 400.0, dt, game) {
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
            (State::Chase | State::Discovery { .. }, _) | (_, 3) => Color::srgb(1.0, 0.1, 0.1),
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
