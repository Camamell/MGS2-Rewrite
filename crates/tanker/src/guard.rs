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
//! Simplified for now: the reload time,
//! close-quarters sub-states, radio reports, body stimuli (noises are
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
        }
    }
}

impl Phase {
    pub fn from_manager(s: Option<&Spawn>) -> Self {
        let mut p = Self::default();
        if let Some(s) = s {
            p.caution_time = s.opt_int(b'h', 0).unwrap_or(0xE10);
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
    /// Saw a comrade down: goes over and wakes him (raising caution).
    Rouse { sleeper: Entity, at: Vec3 },
    /// Held up at gunpoint: hands raised (motion 8). `loose` counts frames
    /// without a gun on him; `shake` counts frames with Snake aiming from
    /// the front (dog tags at [`TAG_FRAMES`]).
    HeldUp { loose: f32, shake: f32 },
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
}

impl Anim {
    /// Gurlukovich soldiers, `gbs.mar`.
    pub const SOLDIER: Anim = Anim { idle: motion::IDLE, walk: motion::WALK, run: motion::RUN, aim: motion::AIM, hands_up: 8, fall: sleep::FALL, lie: sleep::LIE, exclaim: motion::EXCLAIM, look: Some(motion::LOOK), knocked: Some((hurt::FALL, hurt::GET_UP)), get_up: Some(((sleep::GET_UP, 95.0), (sleep::ROUSED, 175.0))) };
    /// US Marines, `ushold.mar`: 39 and 40 cover 1332 and 4057 mm/s, the
    /// soldiers' walk and run; 46 hands up, 151 falls forward, 152 lies.
    /// No rifle-raised motion was found, so they aim from the idle pose.
    pub const MARINE: Anim = Anim { idle: 0, walk: 39, run: 40, aim: 0, hands_up: 46, fall: 151, lie: 152, exclaim: 0, look: None, knocked: None, get_up: None };
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
}

const AIM_FRAMES: f32 = 21.0;
const SHOT_FRAMES: f32 = 4.0;
const MAGAZINE: u32 = 31;
/// Reload pause (the reload motion's length; not read yet).
const RELOAD_FRAMES: f32 = 90.0;
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
        self.path.clear();
    }

    /// Knocked out (punches, a stun grenade): down at once for
    /// [`KNOCKOUT_FRAMES`] and a little more.
    pub fn knock_out(&mut self) {
        // The random 0..63 frames, from his route and node.
        let extra = ((self.route * 37 + self.node * 11) % 64) as f32;
        self.state = State::Asleep { timer: 0.0, wake: KNOCKOUT_FRAMES + extra };
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
    // Comrades lying on the floor (only after they have fallen).
    let sleepers: Vec<(Entity, Vec3)> = guards
        .iter()
        .filter(|(_, g, _)| match g.state {
            State::Asleep { timer, .. } => timer >= sleep::FALL_FRAMES,
            State::Dead { timer } => timer >= sleep::FALL_FRAMES,
            _ => false,
        })
        .map(|(e, _, b)| (e, b.pos))
        .collect();
    let mut woken = Vec::new();
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
    for (me, mut g, mut body) in &mut guards {
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
        if g.level == 3 {
            g.last_seen = Some(snake_pos);
            phase.last_seen = Some(snake_pos);
            g.tracking = Some(0);
        }
        for _ in 0..ticks {
            update_meter(&mut g, phase.kind);
        }
        // A comrade down in sight: go and wake him; the alarm goes to
        // caution.
        if phase.kind != PhaseKind::Alert
            && g.level < 3
            && matches!(g.state, State::Patrol | State::Wait(_) | State::Return | State::Investigate { .. })
        {
            let seen = sleepers.iter().find(|(e, p)| *e != me && perceive(&g, &body, *p, 400.0, &game, 1.0) == 3);
            if let Some(&(sleeper, at)) = seen {
                plan(&mut g, body.pos, at, &game);
                g.state = State::Rouse { sleeper, at };
                if phase.kind == PhaseKind::Infiltration {
                    phase.caution = phase.caution_time;
                    phase.set(PhaseKind::Caution);
                }
            }
        }
        if let State::Rouse { sleeper, at } = g.state {
            if g.level == 3 {
                g.state = State::Discovery { timer: 0 };
            } else {
                body.play(g.anim.run);
                if follow(&mut g, &mut body, RUN_SPEED, 700.0, dt, &game) {
                    face(&mut body, at, dt);
                    woken.push(sleeper);
                    if let Some(wp) = waypoint(&game, g.route, g.node) {
                        plan(&mut g, body.pos, Vec3::from(wp.position), &game);
                    }
                    g.state = State::Return;
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
                    normal_brain(&mut g, &mut body, snake_pos, dt, ticks, &game);
                } else {
                    evasion_brain(&mut g, &mut body, &phase, dt, ticks, &game);
                }
            }
            _ => normal_brain(&mut g, &mut body, snake_pos, dt, ticks, &game),
        }
        highest = highest.max(i32::from(g.meter));
    }

    // Kicked awake: up at once.
    for e in woken {
        if let Ok((_, mut g, _)) = guards.get_mut(e)
            && let State::Asleep { timer, wake } = g.state
            && timer < wake
        {
            g.roused = true;
            g.state = State::Asleep { timer: wake, wake };
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

/// `FUN_00227BC8` + transitions `FUN_00227520` (states the game can
/// currently produce).
fn normal_brain(g: &mut Guard, body: &mut Body, snake: Vec3, dt: f32, ticks: u32, game: &Game) {
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
        State::Asleep { .. } | State::Dead { .. } | State::Rouse { .. } | State::HeldUp { .. } | State::Hurt { .. } => {}
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
        body.play(g.anim.idle);
        face(body, target, dt);
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
            a.reload = RELOAD_FRAMES;
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
