//! Metal Gear Solid 2: the Tanker chapter, rebuilt on Bevy from the data on
//! your own disc.
//!
//! Controls: WASD move (run), Shift walk, C crouch, Space action (doors),
//! Q/E or right mouse drag to turn the camera, F1 collision overlay, F2
//! guard vision cones.

mod body;
mod cbox;
mod bullet;
mod codec;
mod codec_face;
mod collision;
mod contact;
mod crowd;
mod demo;
mod music;
mod sea;
mod sfx;
mod timer;
mod fog;
mod fpv;
mod gcl;
mod grenade;
mod guard;
mod headmark;
mod hud;
mod items;
mod nav;
mod pad;
mod pause;
mod olga;
mod photo;
mod player;
mod radar;
mod rain;
mod save;
mod thunder;
mod title;
mod script_cam;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use bevy::prelude::*;
use clap::Parser;
use mgs2_bevy::scene::{self, MaterialCache, SCALE};
use mgs2_bevy::stage::{self, Disc, StageData};
use mgs2_formats::str_code;

use body::Body;
use collision::Collision;
use guard::{Guard, Phase};
use player::{AutoWalk, FollowCam, Player};

#[derive(Parser)]
#[command(about = "Metal Gear Solid 2 Tanker chapter (needs your SLUS-20144 disc image)")]
struct Cli {
    /// The game's .iso (or set MGS2_ISO).
    #[arg(long)]
    iso: Option<PathBuf>,
    #[arg(default_value = "w00a")]
    stage: String,
    /// Start from the autosave (written each time a stage starts).
    #[arg(long = "continue")]
    resume: bool,
    /// Open the start menu (New game / Continue), then start the game.
    #[arg(long)]
    title: bool,
    /// Start in fullscreen (F11 or Alt+Enter toggles).
    #[arg(long)]
    fullscreen: bool,
    /// Arrive as if coming from this stage (its script supplies Snake's start
    /// and the story marker). Defaults to the opening cutscene `d01t` for
    /// w00a; `none` uses `--entry` instead.
    #[arg(long)]
    from: Option<String>,
    /// Which arrival from `--from` to use (some stages have several).
    #[arg(long, default_value_t = 0)]
    arrival: usize,
    /// Which of the stage's own entry points to start at (without `--from`).
    #[arg(long, default_value_t = 0)]
    entry: usize,
    /// 10 very easy, 20 easy, 30 normal, 40 hard, 50 extreme (the game's
    /// values of script variable Bs16:0x8).
    #[arg(long, default_value_t = 30)]
    difficulty: i16,
    /// Initial camera yaw in degrees.
    #[arg(long, default_value_t = 0.0)]
    cam_yaw: f32,
    /// Camera distance (m) and pitch (degrees above horizontal).
    #[arg(long, default_value_t = 6.5)]
    cam_dist: f32,
    #[arg(long, default_value_t = 60.0)]
    cam_pitch: f32,
    /// Hold "forward" for this many seconds (for automated testing).
    #[arg(long)]
    auto_walk: Option<f32>,
    /// Start the auto-walk this many seconds in.
    #[arg(long, default_value_t = 0.0)]
    walk_from: f32,
    /// Aim the M9 at this time (seconds) and fire half a second later.
    #[arg(long)]
    fire_at: Option<f32>,
    /// Do not show cutscene subtitles (always the case with --screenshot,
    /// so automated runs are not paused).
    #[arg(long)]
    skip_cutscenes: bool,
    /// Show cutscene subtitles even with --screenshot.
    #[arg(long)]
    cutscenes: bool,
    /// Start with the stage cameras off (F3 toggles).
    #[arg(long)]
    no_stage_cams: bool,
    /// Fix the camera at `x,y,z,yaw,pitch` (mm, degrees; yaw 0 = +z), for
    /// screenshots.
    #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
    look: Vec<f32>,
    /// Press Space (grab a ledge / pull up) at this time (seconds).
    #[arg(long)]
    hang_at: Option<f32>,
    /// Knock the boss out at this time (seconds), for tests.
    #[arg(long)]
    boss_down_at: Option<f32>,
    /// Call the Codec (as if K were pressed) at this time (seconds).
    #[arg(long)]
    codec_at: Option<f32>,
    /// Page through Codec calls by themselves (with --codec-at, or for
    /// calls that open on their own).
    #[arg(long)]
    codec_auto: bool,
    /// Press the action button (Space: terminals, doors with `-b`) at
    /// these times (seconds, comma separated).
    #[arg(long, value_delimiter = ',')]
    press_at: Vec<f32>,
    /// Take photos with the digital camera: `t,x,y,z,yaw,pitch;...`
    /// (seconds, mm, degrees; yaw 0 = +z).
    #[arg(long, allow_hyphen_values = true)]
    shots: Option<String>,
    /// How long to aim before that shot (seconds).
    #[arg(long, default_value_t = 0.5)]
    aim_for: f32,
    /// Punch at these times (seconds, comma separated).
    #[arg(long, value_delimiter = ',')]
    punch_at: Vec<f32>,
    /// Put Snake here instead (mm, `--snake-at=x,y,z`; for testing).
    #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
    snake_at: Option<Vec<f32>>,
    /// Snake's facing with `--snake-at` (4096 = full turn, 0 = +z).
    #[arg(long, allow_hyphen_values = true)]
    snake_dir: Option<i32>,
    /// Open the n-th door of the stage after `--door-at` seconds (testing).
    #[arg(long)]
    use_door: Option<usize>,
    #[arg(long, default_value_t = 3.0)]
    door_at: f32,
    /// Face culling: front (default; the strips wind clockwise, and the
    /// PS2 culls, which hides ceilings from the overhead cameras), back or
    /// none.
    #[arg(long, default_value = "front")]
    cull: String,
    /// Start with guard vision cones shown.
    #[arg(long)]
    vision: bool,
    /// Point the camera at the n-th guard instead of Snake.
    #[arg(long)]
    follow_guard: Option<usize>,
    /// Log guard positions and states every second.
    #[arg(long)]
    trace: bool,
    /// Print every actor the stage script creates.
    #[arg(long)]
    spawns: bool,
    /// Save a screenshot after `--shot-at` seconds, then exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    #[arg(long, default_value_t = 8.0)]
    shot_at: f32,
}

#[derive(Resource)]
pub struct Game {
    pub data: StageData,
    pub collision: Option<Collision>,
    pub nav: Option<nav::Nav>,
    pub show_collision: bool,
    pub show_vision: bool,
    /// Script variables as they stood after the stage script ran.
    pub banks: gcl::Banks,
    pub doors: Vec<Door>,
    /// Trap handlers the script registered, and whether Snake is "in" each.
    pub traps: Vec<(gcl::Trap, bool)>,
    /// Snake's start, for continuing after a game over.
    pub snake_start: (Vec3, f32),
    /// Distance (mm) Snake still walks in by himself after coming through a
    /// door (the script's entrance `mesg`); exits arm when it is over.
    pub entrance: f32,
    /// The script camera switched on, if any.
    pub cameras: script_cam::Cameras,
    /// (frequency, CODEC.DAT id) Snake reaches by calling from here.
    pub codec_area: Option<(i32, u32)>,
    /// Incoming calls waiting to open, with the proc to run after each.
    pub codec_calls: Vec<(i32, u32, Option<u32>)>,
    /// Procs of finished calls, to run.
    pub codec_then: Vec<u32>,
    /// The enemy manager's `-a` proc, run when an alert starts (it brings
    /// in reinforcements).
    pub on_alert: Option<u32>,
    /// Troopers on their way: (spawn, seconds until they arrive).
    pub arrivals: Vec<(gcl::Spawn, f32)>,
    /// Rain on the open deck, if the stage has it.
    pub rain: Option<rain::RainSpec>,
    /// The holds' time limit, when the stage runs one.
    pub timer: Option<timer::Timer>,
    /// The stage's fog (`chara #a53aa7`).
    pub fog: Option<fog::FogSpec>,
    /// The radar's turn (radians), when the stage creates one.
    pub radar: Option<f32>,
    /// Lightning, on the open decks.
    pub thunder: Option<thunder::ThunderSpec>,
    /// The script ended the game (`load ... -g`).
    pub game_over: bool,
    /// The variables as the stage started (a continue goes back to them).
    pub start_banks: gcl::Banks,
    /// The crowd's "saw Snake" event has fired.
    pub crowd_alarm: bool,
    /// The crowd actor's `-e` procs (events of the Marines in formation)
    /// and its sight `-t near far` (mm).
    pub crowd_procs: Vec<Option<u32>>,
    pub crowd_sight: (f32, f32),
    /// Spoken line playing: (id, proc to run after, seconds left).
    pub speech: Option<(u32, Option<u32>, f32)>,
    /// Sound driver commands from the script, for `music::update`.
    pub sound_cmds: Vec<u32>,
    /// Digital camera targets and the proc run after each photo.
    pub photos: Vec<gcl::PhotoTarget>,
    pub photo_done: Option<u32>,
    /// Ledges under railings Snake can hang from.
    pub hang_lines: Vec<collision::HangLine>,
    /// Take the trap boxes Snake starts in as already entered (a start that
    /// is not an arrival, so nothing should fire for it).
    pub prime_traps: bool,
}

impl Game {
    /// Marks Snake's entrance as over (bank A 0x7e8 bit 5).
    pub fn entrance_done(&mut self) {
        self.entrance = 0.0;
        self.banks.a[ENTRANCE_DONE.0] |= 1 << ENTRANCE_DONE.1;
    }
}

/// How far Snake walks in through a door on arriving (the scripts pass
/// about this much to the entrance `mesg`).
const ENTRANCE_WALK: f32 = 1500.0;

/// A door actor (`chara 0xebec24`): `-h` gives the two handle points and
/// `-O` the proc run once it is opened (stage exits load the next stage
/// from there).
#[derive(Clone, Debug)]
pub struct Door {
    pub a: Vec3,
    pub b: Vec3,
    pub on_open: u32,
}

const DOOR_ACTOR: u32 = 0xEBEC24;
const PLAYER_ACTOR: u32 = 0x128946;
/// Snake's actor name (the player `chara`'s second argument), which traps
/// name to react to him.
const SNAKE_NAME: u32 = 0x28DCE6;
/// Bank A 0x7e8 bit 5: set by the stage script once Snake's entrance
/// (walking in through the door) is over; exit traps check it.
const ENTRANCE_DONE: (usize, u8) = (0x7E8, 5);
/// How close (mm) Snake must be to a door's handle line to open it.
const DOOR_REACH: f32 = 900.0;

/// Characters to spawn for a freshly loaded stage.
struct Cast {
    snake: Option<(usize, Option<usize>, Vec3, f32)>,
    /// Guards: (guard, model, motions, position, rifle model).
    guards: Vec<(Guard, usize, Option<usize>, Vec3, Option<usize>)>,
    props: Vec<Prop>,
    /// Item boxes and their model.
    items: Vec<(items::Item, usize)>,
    sliding: Vec<(Prop, SlidingDoor)>,
    /// Bosses: (model, motions, position, facing).
    bosses: Vec<(usize, Option<usize>, Vec3, f32, Option<u32>)>,
    /// Marines in formation: (model, motions, position, facing, idle, head
    /// accessory models).
    crowd: Vec<(usize, Option<usize>, Vec3, f32, i32, Vec<usize>)>,
}

/// A static model placed by the script: `chara 0x7a8ec1` (scenery such as
/// floors, furniture and stage-to-stage passages), `0xd1279a` (plants) and
/// `0xcba568` (breakable glass, first model), all with `-m model -r rx ry
/// rz -p x y z [-s sx sy sz]` (angles in 4096ths, scale in percent).
struct Prop {
    model: usize,
    transform: Transform,
    /// Drawn without fog (the distant skyline).
    no_fog: bool,
    /// The lt2 lighting it names (`-l`, scenery only).
    lt2: Option<u32>,
}

const PROP_ACTORS: &[u32] = &[0x7A8EC1, 0xD1279A, 0xCBA568];
const SLIDING_DOOR: u32 = 0x55B942;

/// Deck doors: `-k door handle -p x y z -r rx ry rz`, both models drawn in
/// place (they are not animated yet; opening one runs its exit at once).
fn door_props(s: &gcl::Spawn, data: &StageData) -> Vec<Prop> {
    let v = |l: u8, i: usize| s.opt_int(l, i).unwrap_or(0) as f32;
    let angle = |i| v(b'r', i) / 4096.0 * std::f32::consts::TAU;
    let transform = Transform {
        translation: Vec3::new(v(b'p', 0), v(b'p', 1), v(b'p', 2)) * SCALE,
        rotation: Quat::from_euler(EulerRot::YXZ, angle(1), angle(0), angle(2)),
        scale: Vec3::ONE,
    };
    s.opt(b'k')
        .unwrap_or(&[])
        .iter()
        .filter_map(|m| {
            let hash = m.int()? as u32 & 0x00FF_FFFF;
            let model = data.models.iter().position(|(h, _)| *h & 0x00FF_FFFF == hash)?;
            Some(Prop { model, transform, no_fog: false, lt2: None })
        })
        .collect()
}

/// A sliding door (`chara 0x55b942 -m model -d rx ry rz -p x y z -s width`):
/// opens sideways when someone comes near.
#[derive(Component)]
pub struct SlidingDoor {
    closed: Transform,
    /// Local slide direction times the width (metres).
    slide: Vec3,
    /// 0 closed .. 1 open.
    open: f32,
}

fn sliding_door(s: &gcl::Spawn, data: &StageData) -> Option<(Prop, SlidingDoor)> {
    let hash = s.opt(b'm')?.first()?.int()? as u32 & 0x00FF_FFFF;
    let model = data.models.iter().position(|(h, _)| *h & 0x00FF_FFFF == hash)?;
    let v = |l: u8, i: usize| s.opt_int(l, i).unwrap_or(0) as f32;
    let angle = |i| v(b'd', i) / 4096.0 * std::f32::consts::TAU;
    let transform = Transform {
        translation: Vec3::new(v(b'p', 0), v(b'p', 1), v(b'p', 2)) * SCALE,
        rotation: Quat::from_euler(EulerRot::YXZ, angle(1), angle(0), angle(2)),
        scale: Vec3::ONE,
    };
    let width = s.opt_int(b's', 0).filter(|w| *w > 0).unwrap_or(1000) as f32 * SCALE;
    Some((Prop { model, transform, no_fog: false, lt2: None }, SlidingDoor { closed: transform, slide: transform.rotation * Vec3::X * width, open: 0.0 }))
}

/// Sliding doors open for anyone within 2 m and close again after.
fn slide_doors(
    time: Res<Time>,
    bodies: Query<&Body, Without<SlidingDoor>>,
    mut doors: Query<(&mut SlidingDoor, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut d, mut t) in &mut doors {
        let at = d.closed.translation / SCALE;
        let near = bodies.iter().any(|b| Vec2::new(b.pos.x - at.x, b.pos.z - at.z).length() < 2000.0 && (b.pos.y - at.y).abs() < 1500.0);
        let step = if near { dt * 2.5 } else { -dt * 2.5 };
        d.open = (d.open + step).clamp(0.0, 1.0);
        t.translation = d.closed.translation + d.slide * d.open;
    }
}

/// The New York skyline seen from the deck: `manhat3d.c` (`#1e44b7`,
/// `FUN_00305db8`) puts six copies of model 0x198aa2 at x -130000, then
/// -160000 in 20 m steps; `brooklyn.c` (`#d6ed95`, `FUN_00306748`) six of
/// 0x198b62 at x 130000, then 150000 in 10 m steps (y from `-h`, else 0;
/// z 0). The models are 1:128 miniatures (the actors keep a 128 scale); the
/// skyline is drawn at the same angle of view but closer: scaled and moved
/// out by `SKYLINE_SCALE`, within the camera's far plane. Their per-frame
/// movement is not ported.
fn skyline(s: &gcl::Spawn, data: &StageData) -> Vec<Prop> {
    let (model, xs): (u32, [f32; 6]) = match s.kind {
        0x1E44B7 => (0x198AA2, [-130000.0, -160000.0, -180000.0, -200000.0, -220000.0, -240000.0]),
        0xD6ED95 => (0x198B62, [130000.0, 150000.0, 160000.0, 170000.0, 180000.0, 190000.0]),
        _ => return Vec::new(),
    };
    let Some(idx) = data.models.iter().position(|(h, _)| *h & 0x00FF_FFFF == model) else { return Vec::new() };
    let y = s.opt_int(b'h', 0).unwrap_or(0) as f32;
    xs.iter()
        .map(|x| Prop {
            model: idx,
            transform: Transform::from_translation(Vec3::new(*x, y, 0.0) * SKYLINE_SCALE * SCALE).with_scale(Vec3::splat(SKYLINE_SCALE)),
            no_fog: true,
            lt2: None,
        })
        .collect()
}

const SKYLINE_SCALE: f32 = 6.0;

fn prop(s: &gcl::Spawn, data: &StageData) -> Option<Prop> {
    // `-f` bit 0x100 marks helpers that are never drawn (e.g. the stretched
    // `w04_test_bou` poles).
    if s.opt_int(b'f', 0).is_some_and(|f| f & 0x100 != 0) {
        return None;
    }
    let hash = s.opt(b'm')?.first()?.int()? as u32 & 0x00FF_FFFF;
    let model = data.models.iter().position(|(h, _)| *h & 0x00FF_FFFF == hash)?;
    let v = |l: u8, i: usize, d: i32| s.opt_int(l, i).unwrap_or(d) as f32;
    let angle = |i| v(b'r', i, 0) / 4096.0 * std::f32::consts::TAU;
    let transform = Transform {
        translation: Vec3::new(v(b'p', 0, 0), v(b'p', 1, 0), v(b'p', 2, 0)) * SCALE,
        rotation: Quat::from_euler(EulerRot::YXZ, angle(1), angle(0), angle(2)),
        scale: Vec3::new(v(b's', 0, 100), v(b's', 1, 100), v(b's', 2, 100)) / 100.0,
    };
    let lt2 = match s.opt(b'l').and_then(|l| l.first()) {
        Some(gcl::Val::Str(name)) => Some(str_code(name.as_bytes())),
        Some(v) if s.kind == 0x7A8EC1 => v.int().map(|h| h as u32),
        _ => None,
    };
    Some(Prop { model, transform, no_fog: false, lt2 })
}

/// A stage ready to be put into the world.
struct Level {
    game: Game,
    phase: Phase,
    cast: Cast,
}

/// Everything that belongs to the current stage; despawned on a change.
#[derive(Component, Clone)]
struct LevelEntity;

/// Set while a newly loaded stage waits to be spawned.
#[derive(Resource)]
struct NewCast(Mutex<Option<Cast>>);

#[derive(Resource)]
struct Session {
    disc: Mutex<Disc>,
    difficulty: i16,
    print_spawns: bool,
    show_vision: bool,
    use_door: Option<(usize, f32)>,
    trace_traps: bool,
    iso: PathBuf,
    skip_cutscenes: bool,
    cull: Option<bevy::render::render_resource::Face>,
}

#[derive(Resource)]
struct Start {
    cam_yaw: f32,
    cam_dist: f32,
    cam_pitch: f32,
    follow_guard: Option<usize>,
    no_stage_cams: bool,
}

#[derive(Resource)]
struct Capture {
    path: PathBuf,
    at: f32,
    taken: bool,
}

#[derive(Component)]
struct Hud;

fn model_index(data: &StageData, name: &str) -> Option<usize> {
    data.models.iter().position(|(h, _)| *h == str_code(name.as_bytes()))
}

fn motions_index(data: &StageData, name: &str) -> Option<usize> {
    data.motions.iter().position(|(h, _)| *h == str_code(name.as_bytes()))
}

fn set_snake_start(banks: &mut gcl::Banks, pos: [f32; 3], dir: i32) {
    for (off, v) in [(gcl::SNAKE_X, pos[0] as i32), (gcl::SNAKE_Y, pos[1] as i32), (gcl::SNAKE_Z, pos[2] as i32), (gcl::SNAKE_DIR, dir)] {
        banks.set_a32(off, v);
    }
}

/// Loads a stage and runs its script with `banks` (which carry Snake's
/// start position and the story state), then works out the cast.
/// `walk_in`: Snake comes through a door and first walks in.
fn prepare(
    disc: &mut Disc,
    name: &str,
    banks: gcl::Banks,
    walk_in: bool,
    print_spawns: bool,
    show_vision: bool,
) -> Result<Level> {
    let mut banks = banks;
    banks.set_b32(gcl::STAGE_NAME, mgs2_formats::str_code(name.as_bytes()) as i32);
    let mut data = stage::load(disc, name)?;
    let collision = data.collision.as_ref().map(Collision::new);
    let nav = data.collision.as_ref().map(nav::Nav::new);

    let mut guards = Vec::new();
    let mut phase = Phase::default();
    let mut doors = Vec::new();
    let mut traps = Vec::new();
    let mut props = Vec::new();
    let mut items = Vec::new();
    let mut sliding = Vec::new();
    let mut bosses = Vec::new();
    let mut crowd = Vec::new();
    let mut on_alert = None;
    let mut rain_spec = None;
    let mut holds_timer = None;
    let mut fog_spec = None;
    let mut radar_turn = None;
    let mut thunder_spec = None;
    let mut crowd_procs = (Vec::new(), (2500.0, 7000.0));
    let mut cameras = script_cam::Cameras::default();
    let mut codec_reqs = Vec::new();
    let mut photos = (Vec::new(), None);
    let mut banks_after = banks.clone();
    // Snake's start: the player actor's `-p x y z -d dir`, which read the
    // start variables; else the start variables themselves.
    let a32 = |o: u16| banks.a32(o);
    let mut start = (
        Vec3::new(a32(gcl::SNAKE_X) as f32, a32(gcl::SNAKE_Y) as f32, a32(gcl::SNAKE_Z) as f32),
        a32(gcl::SNAKE_DIR),
    );
    if let Some(script) = &data.script {
        let mut vm = gcl::Vm::with_banks(script, banks);
        vm.run(&script.main);
        let (gm, gmar) = (model_index(&data, "gbs_def"), motions_index(&data, "gbs"));
        // An actor created again under the same name replaces the first.
        let actor = |s: &gcl::Spawn| s.args.first().and_then(|v| v.int());
        let soldiers: Vec<&gcl::Spawn> = vm.spawns.iter().filter(|s| s.kind == guard::GUARD_ACTOR).collect();
        let last = |i: usize| soldiers[i + 1..].iter().all(|o| actor(o) != actor(soldiers[i]));
        for s in (0..soldiers.len()).filter(|i| last(*i)).map(|i| soldiers[i]) {
            if let (Some((g, mut p)), Some(m)) = (guard::from_spawn(s, 6000.0), gm) {
                if let Some(col) = &collision
                    && let Some(y) = col.floor_below(p.x, p.z, p.y + 1000.0) {
                        p.y = y;
                    }
                guards.push((g, m, gmar, p, model_index(&data, "aks")));
            }
        }
        for s in vm.spawns.iter().filter(|s| s.kind == DOOR_ACTOR) {
            let h = |i| s.opt_int(b'h', i).unwrap_or(0) as f32;
            if let Some(gcl::Val::Proc(p)) = s.opt(b'O').and_then(|v| v.first()) {
                doors.push(Door { a: Vec3::new(h(0), h(1), h(2)), b: Vec3::new(h(3), h(4), h(5)), on_open: *p });
            }
        }
        if let Some(p) = vm.spawns.iter().find(|s| s.kind == PLAYER_ACTOR) {
            let (x, y, z) = (p.opt_int(b'p', 0), p.opt_int(b'p', 1), p.opt_int(b'p', 2));
            if let (Some(x), Some(y), Some(z)) = (x, y, z) {
                start = (Vec3::new(x as f32, y as f32, z as f32), p.opt_int(b'd', 1).unwrap_or(start.1));
            }
        }
        rain_spec = vm.spawns.iter().find(|s| s.kind == rain::RAIN_ACTOR).and_then(rain::from_spawn);
        holds_timer = vm.spawns.iter().find(|s| s.kind == timer::TIMER_ACTOR).and_then(timer::Timer::from_spawn);
        fog_spec = vm.spawns.iter().filter(|s| s.kind == fog::FOG_ACTOR).find_map(fog::from_spawn);
        radar_turn = vm.spawns.iter().find(|s| s.kind == radar::RADAR_ACTOR).map(radar::angle);
        thunder_spec = vm.spawns.iter().find(|s| s.kind == thunder::THUNDER_ACTOR).map(thunder::from_spawn);
        let manager = vm.spawns.iter().find(|s| s.kind == guard::MANAGER_ACTOR);
        phase = Phase::from_manager(manager);
        on_alert = manager.and_then(|s| match s.opt(b'a')?.first()? {
            gcl::Val::Proc(p) => Some(*p),
            _ => None,
        });
        traps = vm.traps.iter().map(|t| (t.clone(), false)).collect();
        props = vm.spawns.iter().filter(|s| PROP_ACTORS.contains(&s.kind)).filter_map(|s| prop(s, &data)).collect();
        props.extend(vm.spawns.iter().filter(|s| s.kind == DOOR_ACTOR).flat_map(|s| door_props(s, &data)));
        props.extend(vm.spawns.iter().flat_map(|s| skyline(s, &data)));
        sliding = vm.spawns.iter().filter(|s| s.kind == SLIDING_DOOR).filter_map(|s| sliding_door(s, &data)).collect();
        bosses = vm
            .spawns
            .iter()
            .filter(|s| s.kind == olga::OLGA_ACTOR)
            .filter_map(|s| olga::from_spawn(s).map(|x| (x, olga::callbacks(s).get(1).copied())))
            .filter_map(|((m, a, mut p, yaw), on_down)| {
                let model = data.models.iter().position(|(h, _)| *h & 0x00FF_FFFF == m)?;
                let motions = data.motions.iter().position(|(h, _)| *h & 0x00FF_FFFF == a);
                if let Some(y) = collision.as_ref().and_then(|c| c.floor_below(p.x, p.z, p.y + 100.0)) {
                    p.y = y;
                }
                Some((model, motions, p, yaw, on_down))
            })
            .collect();
        let ushold = motions_index(&data, "ushold");
        // The Marines who patrol (crowd members on a route).
        let vision = banks_vision(&vm.banks());
        if let Some(m) = model_index(&data, "us_def_1") {
            for route in vm.spawns.iter().filter(|s| s.kind == crowd::CROWD_ACTOR).flat_map(crowd::walkers) {
                let start = data.collision.as_ref().and_then(|h| h.routes.get(route)).and_then(|r| r.points.first());
                if let Some(wp) = start {
                    let mut p = Vec3::from(wp.position);
                    if let Some(y) = collision.as_ref().and_then(|c| c.floor_below(p.x, p.z, p.y + 1000.0)) {
                        p.y = y;
                    }
                    guards.push((guard::patrol(route, vision, guard::Anim::MARINE), m, ushold, p, model_index(&data, "m4a_nm")));
                }
            }
        }
        if let Some(s) = vm.spawns.iter().find(|s| s.kind == crowd::CROWD_ACTOR) {
            crowd_procs = crowd::events(s);
        }
        let members: Vec<crowd::Member> = vm.spawns.iter().filter(|s| s.kind == crowd::CROWD_ACTOR).flat_map(crowd::members).collect();
        // Each Marine variant once, built from the part library.
        let mut variants: HashMap<i32, usize> = HashMap::new();
        if let (Some(base), Some(lib), Some(img)) = (model_index(&data, "us_def_1"), model_index(&data, "w04a_us_def"), data.resident.clone()) {
            for v in members.iter().map(|m| m.variant).filter(|v| *v < crowd::PART_VARIANTS) {
                if variants.contains_key(&v) {
                    continue;
                }
                if let Some(model) = crowd::variant_model(&data.models[base].1, &data.models[lib].1, &img, v) {
                    data.models.push((0xFF00_0000 | v as u32, model));
                    variants.insert(v, data.models.len() - 1);
                }
            }
        }
        crowd = members
            .iter()
            .filter_map(|m| {
                let model = variants.get(&m.variant).copied().or_else(|| model_index(&data, crowd::model_name(m.variant)))?;
                let extras = crowd::accessories(m.flags).into_iter().filter_map(|n| model_index(&data, n)).collect();
                Some((model, ushold, m.pos, m.yaw, m.idle, extras))
            })
            .collect();
        items = vm
            .spawns
            .iter()
            .filter(|s| s.kind == items::ITEM_ACTOR)
            .filter_map(items::Item::from_spawn)
            .filter_map(|(item, model)| Some((item, data.models.iter().position(|(h, _)| *h & 0x00FF_FFFF == model)?)))
            .collect();
        eprintln!(
            "{name}: script created {} actors, {} guards, {} doors, {} traps, {} props, {} of {} items",
            vm.spawns.len(),
            guards.len(),
            doors.len(),
            traps.len(),
            props.len(),
            items.len(),
            vm.spawns.iter().filter(|s| s.kind == items::ITEM_ACTOR).count(),
        );
        if print_spawns {
            for s in &vm.spawns {
                eprintln!("  {:06x} {:?} {:?}", s.kind, s.args, s.options);
            }
        }
        cameras.apply(&vm.spawns);
        codec_reqs = vm.codec.clone();
        photos = (std::mem::take(&mut vm.photos), vm.photo_done);
        if !photos.0.is_empty() {
            eprintln!("{name}: {} photo targets", photos.0.len());
        }
        banks_after = vm.banks();
    }

    let mut pos = start.0;
    if let Some(col) = &collision
        && let Some(y) = col.floor_below(pos.x, pos.z, pos.y + 2000.0) {
            pos.y = y;
        }
    let yaw = start.1 as f32 / 4096.0 * std::f32::consts::TAU;
    let snake = model_index(&data, "sna_def").map(|m| (m, motions_index(&data, "snake"), pos, yaw));
    let mut game = Game {
        data,
        collision,
        nav,
        show_collision: false,
        show_vision,
        banks: banks_after,
        doors,
        traps,
        snake_start: (pos, yaw),
        entrance: 0.0,
        prime_traps: false,
        cameras,
        codec_area: None,
        codec_calls: Vec::new(),
        codec_then: Vec::new(),
        on_alert,
        arrivals: Vec::new(),
        rain: rain_spec,
        timer: holds_timer,
        fog: fog_spec,
        radar: radar_turn,
        thunder: thunder_spec,
        game_over: false,
        start_banks: gcl::Banks::default(),
        crowd_alarm: false,
        crowd_procs: crowd_procs.0,
        crowd_sight: crowd_procs.1,
        speech: None,
        sound_cmds: Vec::new(),
        photos: photos.0,
        photo_done: photos.1,
        hang_lines: Vec::new(),
    };
    if !game.photos.is_empty() {
        photo::init_slots(&mut game.banks);
    }
    game.start_banks = game.banks.clone();
    apply_codec(&mut game, &codec_reqs);
    if let (Some(h), Some(c)) = (&game.data.collision, &game.collision) {
        game.hang_lines = collision::hang_lines(h, c);
    }
    if walk_in {
        game.entrance = ENTRANCE_WALK;
    } else {
        game.entrance_done();
    }
    Ok(Level { game, phase, cast: Cast { snake, guards, props, items, sliding, bosses, crowd } })
}

fn main() -> Result<()> {
    let mut cli = Cli::parse();
    if cli.title {
        let mut pass = Vec::new();
        if let Some(iso) = &cli.iso {
            pass.extend(["--iso".to_string(), iso.display().to_string()]);
        }
        if let Some(args) = title::run(pass, cli.difficulty, cli.fullscreen) {
            std::process::Command::new(std::env::current_exe()?).args(args).spawn().context("starting the game")?;
        }
        return Ok(());
    }
    let saved = if cli.resume { save::read() } else { None };
    match &saved {
        Some(s) => {
            info!("continuing at {}", s.stage);
            cli.stage = s.stage.clone();
            cli.from = Some("none".into());
        }
        None if cli.resume => eprintln!("no save at {}; starting {}", save::path().display(), cli.stage),
        None => {}
    }
    let iso = cli.iso.or_else(stage::default_iso).context("pass --iso <path to MGS2 .iso> or set MGS2_ISO")?;
    let mut disc = stage::open_disc(&iso)?;
    let codec_data = codec::CodecData::load(&iso).map_err(|e| eprintln!("CODEC.DAT not loaded: {e:#}")).ok();

    // Snake's start: how the previous stage sends him here, else one of this
    // stage's own entry points.
    let mut banks = gcl::Banks::default();
    banks.set_b16(gcl::DIFFICULTY, cli.difficulty);
    // The title screen's script opens with the per-difficulty settings
    // (guard vision ranges As16:0x1bc.., timers); run just that statement.
    if let Some(title) = stage::script(&mut disc, "n_title")?
        && let Some(first) = title.main.first() {
            let mut vm = gcl::Vm::with_banks(&title, banks);
            vm.run(std::slice::from_ref(first));
            // A new game's variables (`#b22659 difficulty`): story flags, the
            // holds' time limit Au32:0x70, the Marines' tables...
            vm.call_proc(0xB2_2659, &[i32::from(cli.difficulty)]);
            banks = vm.banks();
        }
    let from = cli.from.clone().or_else(|| (cli.stage == "w00a").then(|| "d01t".to_string()));
    let arrival = match from.as_deref() {
        Some("none") | None => None,
        Some(f) => stage::arrivals(&mut disc, f, &cli.stage)?.get(cli.arrival).copied(),
    };
    if let Some(p) = arrival.and_then(|a| a.progress) {
        banks.set_a32(gcl::PROGRESS, p as i32);
    }
    let entry = match arrival {
        Some(a) => Some(a.entry),
        // The stage's own entries need its data; peek at them first.
        None => stage::load(&mut disc, &cli.stage)?.entries.get(cli.entry).copied(),
    };
    if let Some(e) = entry {
        set_snake_start(&mut banks, e.position, e.direction);
    }
    if let Some(at) = cli.snake_at.as_ref().filter(|a| a.len() == 3) {
        let dir = cli.snake_dir.unwrap_or(entry.map_or(0, |e| e.direction));
        set_snake_start(&mut banks, [at[0], at[1], at[2]], dir);
    }
    // Coming from another gameplay stage (not a cutscene `d..t`), Snake
    // walks in through the door.
    // Continuing: the variables as the saved stage started.
    if let Some(s) = &saved {
        banks = s.banks.clone();
    }
    let walk_in = arrival.is_some() && from.as_deref().is_some_and(|f| f.starts_with('w'));
    let mut level = prepare(&mut disc, &cli.stage, banks, walk_in, cli.spawns, cli.vision)?;
    // The cutscene Snake comes from (the opening briefing for w00a).
    let skip_cutscenes = cli.skip_cutscenes || (cli.screenshot.is_some() && !cli.cutscenes);
    let opening = match from.as_deref() {
        Some(f) if f.starts_with('d') && f.ends_with('t') && !skip_cutscenes => {
            stage::script(&mut disc, f)?.map(|g| {
                let data = stage::load(&mut disc, f).ok();
                let scenery = data.as_ref().map(|d| cutscene_scenery(&g, d, &level.game.banks)).unwrap_or_default();
                let mut c = demo::load(&iso, f, &g, data);
                c.scenery = scenery;
                c
            })
        }
        _ => None,
    };
    level.game.prime_traps = arrival.is_none();
    // --snake-at wins over a script that places Snake itself.
    if let (Some(at), Some(snake)) = (cli.snake_at.as_ref().filter(|a| a.len() == 3), level.cast.snake.as_mut()) {
        let mut pos = Vec3::new(at[0], at[1], at[2]);
        if let Some(y) = level.game.collision.as_ref().and_then(|c| c.floor_below(pos.x, pos.z, pos.y + 2000.0)) {
            pos.y = y;
        }
        let yaw = cli.snake_dir.map_or(snake.3, |d| d as f32 / 4096.0 * std::f32::consts::TAU);
        (snake.2, snake.3) = (pos, yaw);
        level.game.snake_start = (pos, yaw);
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "MGS2 Tanker".into(),
            mode: if cli.fullscreen {
                bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
            } else {
                bevy::window::WindowMode::Windowed
            },
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.02, 0.03, 0.05)))
    .insert_resource(level.game)
    .insert_resource(level.phase)
    .insert_resource(NewCast(Mutex::new(Some(level.cast))))
    .insert_resource(bullet::Rng(0x2001_1113))
    .insert_resource(match &saved {
        Some(s) => items::Inventory { counts: s.counts.clone(), usp: s.usp, weapon: s.weapon, ..default() },
        None => items::Inventory::default(),
    })
        .init_resource::<guard::Gunshot>()
    .init_resource::<contact::Punches>()
    .init_resource::<codec::CodecState>()
    .insert_resource(opening.unwrap_or_default())
    .insert_resource(Session {
        disc: Mutex::new(disc),
        difficulty: cli.difficulty,
        print_spawns: cli.spawns,
        show_vision: cli.vision,
        use_door: cli.use_door.map(|d| (d, cli.door_at)),
        trace_traps: cli.trace,
        iso: iso.clone(),
        skip_cutscenes,
        cull: match cli.cull.as_str() {
            "back" => Some(bevy::render::render_resource::Face::Back),
            "front" => Some(bevy::render::render_resource::Face::Front),
            _ => None,
        },
    })
    .insert_resource(Start {
        cam_yaw: cli.cam_yaw.to_radians(),
        cam_dist: cli.cam_dist,
        cam_pitch: cli.cam_pitch.to_radians(),
        follow_guard: cli.follow_guard,
        no_stage_cams: cli.no_stage_cams,
    })
    .add_systems(Startup, (setup, codec::setup_screen, demo::setup_screen))
    .add_systems(
        Update,
        (
            spawn_level.run_if(resource_exists::<NewCast>),
            player::control,
            items::pickup,
            contact::punch,
            guard::think,
            contact::separate,
            bullet::fly,
            items::sync_rations,
            game_over,
            body::skin,
            player::two_sided_rays,
            player::follow_camera,
            toggles,
            doors,
            traps,
            slide_doors,
            draw_collision,
            guard::draw,
            bullet::draw,
            hud,
        )
            .chain()
            .run_if(codec::idle)
            .run_if(demo::idle),
    );
    app.add_systems(Update, (codec::input, codec::screen, codec::voice).chain().run_if(demo::idle));
    app.add_systems(Update, (demo::play, demo::stage, demo::animate, demo::sound, demo::fade).chain().after(player::follow_camera));
    app.add_plugins(music::plugin);
    app.add_plugins(radar::plugin);
    app.add_plugins(headmark::plugin);
    app.add_plugins(fpv::plugin);
    app.add_plugins(hud::plugin);
    app.add_systems(Update, player::roll_hits.after(player::control));
    // Test runs (--screenshot, the pose sheets, MGS2_NO_SAVE) leave the
    // player's save alone.
    let testing = ["MGS2_GUARD_SHEET", "MGS2_MOTION_SHEET", "MGS2_NO_SAVE"].iter().any(|v| std::env::var_os(v).is_some());
    if cli.screenshot.is_none() && !testing {
        app.add_systems(Update, save::autosave);
    }
    app.add_plugins((pad::plugin, pause::plugin));
    app.add_plugins(cbox::plugin);
    app.add_plugins(grenade::plugin);
    app.add_systems(Update, motion_sheet.after(player::control).before(body::skin));
    app.add_systems(Update, guard_sheet.after(guard::think).after(olga::think).before(body::skin));
    app.add_plugins(thunder::plugin);
    app.add_plugins(thunder::bolt_plugin);
    // MGS2_FPS=1: log frame rate and time every second.
    if std::env::var_os("MGS2_FPS").is_some() {
        app.add_plugins((bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(), bevy::diagnostic::LogDiagnosticsPlugin::default()));
        app.add_systems(Update, |time: Res<Time<Real>>, mut next: Local<f32>, bodies: Query<(), With<body::Body>>| {
            if time.elapsed_secs() >= *next {
                *next = time.elapsed_secs() + 1.0;
                use std::sync::atomic::Ordering::Relaxed;
                info!("skinned {} bodies, {} vertices this second; {} bodies", body::BODIES.swap(0, Relaxed), body::STATS.swap(0, Relaxed), bodies.iter().count());
            }
        });
    }
    app.add_systems(Update, (fog::apply, fog::unfog));
    app.add_systems(Startup, codec_face::setup_portraits).add_systems(Update, (codec_face::load, codec_face::show, codec_face::animate.after(codec_face::load)));
    if let [x, y, z, yaw, pitch] = cli.look[..] {
        let tf = Transform::from_translation(Vec3::new(x, y, z) * mgs2_bevy::scene::SCALE)
            .with_rotation(Quat::from_euler(EulerRot::YXZ, (yaw + 180.0).to_radians(), pitch.to_radians(), 0.0));
        app.add_systems(PostUpdate, (move |mut cam: Query<&mut Transform, With<FollowCam>>| {
            for mut t in &mut cam {
                *t = tf;
            }
        }).before(bevy::transform::TransformSystems::Propagate));
    }
    app.add_systems(Update, (sea::setup, sea::animate.after(player::follow_camera)));
    app.add_systems(Update, sfx::footsteps.run_if(demo::idle).run_if(codec::idle));
    app.init_resource::<photo::PhotoMode>()
        .add_systems(Startup, photo::setup_hud)
        .add_systems(
            Update,
            (photo::control.before(player::control), photo::view.after(player::follow_camera), codec_followup.after(codec::input), speech, holds_timer, reinforcements.after(guard::think), snake_vars, crowd_watch.after(snake_vars))
                .run_if(codec::idle)
                .run_if(demo::idle),
        )
        .add_systems(Update, photo::hud)
        .add_systems(Update, (rain::setup, rain::fall).chain().after(demo::animate))
        .add_systems(Update, chapter_end.run_if(demo::idle));
    if let Some(c) = codec_data {
        app.insert_resource(c);
    }
    app.add_systems(Update, body::hold.after(body::skin).before(player::follow_camera));
    app.add_systems(Update, (body::add_shadows, body::update_shadows.after(body::skin)));
    app.add_systems(Update, boss_defeat.after(olga::think).run_if(codec::idle).run_if(demo::idle));
    app.add_systems(Update, olga::think.after(guard::think).before(bullet::fly).run_if(codec::idle).run_if(demo::idle));
    if cli.trace {
        app.insert_resource(guard::Trace::default()).add_systems(Update, guard::trace);
    }
    if let Some(s) = cli.auto_walk {
        app.insert_resource(AutoWalk { from: cli.walk_from, seconds: s });
    }
    if let Some(t) = cli.fire_at {
        app.insert_resource(player::AutoFire(t, cli.aim_for));
    }
    if let Some(t) = cli.hang_at {
        app.insert_resource(player::AutoHang(t));
    }
    if let Some(t) = cli.boss_down_at {
        app.insert_resource(olga::DownAt(t));
    }
    if cli.codec_at.is_some() || cli.codec_auto {
        app.insert_resource(codec::AutoCall(cli.codec_at.unwrap_or(f32::MAX), cli.codec_auto));
    }
    if !cli.press_at.is_empty() {
        app.insert_resource(AutoPress(cli.press_at.clone()));
    }
    if let Some(s) = &cli.shots {
        app.insert_resource(photo::AutoShots(photo::parse_shots(s), None));
    }
    if !cli.punch_at.is_empty() {
        app.insert_resource(contact::AutoPunch(cli.punch_at.clone()));
    }
    if let Some(path) = cli.screenshot {
        app.insert_resource(Capture { path, at: cli.shot_at, taken: false }).add_systems(Last, capture);
    }
    app.run();
    Ok(())
}

/// Camera, light and HUD: these live across stages.
fn setup(mut commands: Commands, start: Res<Start>) {
    commands.spawn((
        Camera3d::default(),
        Transform::default(),
        FollowCam {
            yaw: start.cam_yaw,
            pitch: start.cam_pitch,
            distance: start.cam_dist,
            current: start.cam_dist,
            scripted: !start.no_stage_cams,
            look: Vec3::ZERO,
            guard: start.follow_guard,
        },
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 400.0, ..default() });
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() },
        // Wraps short of the radar in the top-right corner.
        Node { position_type: PositionType::Absolute, top: px(8), left: px(8), right: px(250), ..default() },
    ));
}

/// Puts the current stage's level geometry and cast into the world.
#[allow(clippy::too_many_arguments)]
fn spawn_level(
    mut commands: Commands,
    game: Res<Game>,
    session: Res<Session>,
    new: Res<NewCast>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cams: Query<&mut FollowCam>,
    mut first: Local<bool>,
    inventory: Res<items::Inventory>,
) {
    let cast = new.0.lock().unwrap().take();
    // After a stage change, look over Snake's shoulder the way he faces.
    if std::mem::replace(&mut *first, true)
        && let (Some((_, _, _, yaw)), Ok(mut cam)) = (cast.as_ref().and_then(|c| c.snake), cams.single_mut()) {
            cam.yaw = yaw + std::f32::consts::PI;
        }
    commands.remove_resource::<NewCast>();
    let Some(cast) = cast else { return };
    let mut cache = MaterialCache { cull: session.cull, ..default() };
    let unlit = true;
    let data = &game.data;
    scene::spawn_level(&mut commands, data, &mut cache, &mut meshes, &mut images, &mut materials, unlit, LevelEntity);
    for p in cast.props {
        let model = &data.models[p.model].1;
        // The skyline far off is not lit by the ship's lights.
        let e = if p.no_fog {
            scene::spawn_model(&mut commands, model, data, &mut cache, &mut meshes, &mut images, &mut materials, unlit, LevelEntity)
        } else {
            scene::spawn_model_lit(&mut commands, model, data, &mut cache, &mut meshes, &mut images, &mut materials, unlit, LevelEntity, p.transform, p.lt2)
        };
        commands.entity(e).insert(p.transform);
        if p.no_fog {
            commands.entity(e).insert(fog::NoFog);
        }
    }
    for (p, door) in cast.sliding {
        let model = &data.models[p.model].1;
        let e = scene::spawn_model_lit(&mut commands, model, data, &mut cache, &mut meshes, &mut images, &mut materials, unlit, LevelEntity, p.transform, p.lt2);
        commands.entity(e).insert((p.transform, door));
    }
    for (item, model) in cast.items {
        if inventory.taken.contains(&(data.name.clone(), item.name)) {
            continue;
        }
        let model = &data.models[model].1;
        let at = Transform::from_translation(item.pos * SCALE);
        let e = scene::spawn_model_lit(&mut commands, model, data, &mut cache, &mut meshes, &mut images, &mut materials, unlit, LevelEntity, at, None);
        commands.entity(e).insert((at, item));
    }
    let mut spawn = |commands: &mut Commands, model: usize, bundle: Body| {
        body::spawn_body(
            commands,
            &data.models[model].1,
            data,
            &mut cache,
            &mut meshes,
            &mut images,
            &mut materials,
            unlit,
            (bundle, LevelEntity),
        )
    };
    // (owner, model, shown only while aiming, orientation, joint)
    let mut weapons = Vec::new();
    let mut usp_model = None;
    match cast.snake {
        Some((model, motions, pos, yaw)) => {
            let e = spawn(&mut commands, model, Body::new(pos, yaw, model, motions));
            commands.entity(e).insert(Player::new(session.difficulty));
            weapons.extend(model_index(data, "m92").map(|m| (e, m, true, body::pistol_grip(), body::RIGHT_HAND)));
            usp_model = model_index(data, "usp").map(|m| (e, m));
        }
        None => error!("Snake's model (sna_def) not found"),
    }
    for (g, model, motions, pos, gun) in cast.guards {
        let e = spawn(&mut commands, model, Body::new(pos, 0.0, model, motions));
        commands.entity(e).insert(g);
        weapons.extend(gun.map(|m| (e, m, false, body::rifle_grip(), body::RIGHT_HAND)));
    }
    for (model, motions, pos, yaw, on_down) in cast.bosses {
        let e = spawn(&mut commands, model, Body::new(pos, yaw, model, motions));
        commands.entity(e).insert(olga::Olga::new(on_down));
    }
    for (model, motions, pos, yaw, idle, extras) in cast.crowd {
        let mut b = Body::new(pos, yaw, model, motions);
        b.frame = idle as f32 * 7.0;
        let e = spawn(&mut commands, model, b);
        commands.entity(e).insert(crowd::Marine);
        weapons.extend(extras.into_iter().map(|m| (e, m, false, Quat::IDENTITY, crowd::HEAD)));
    }
    for (owner, model, when_aiming, grip, object) in weapons {
        let m = &data.models[model].1;
        let e = scene::spawn_model(&mut commands, m, data, &mut cache, &mut meshes, &mut images, &mut materials, unlit, LevelEntity);
        // Snake's M9 shows only with the M9 in hand.
        let weapon = when_aiming.then_some(0);
        commands.entity(e).insert(body::Held { owner, object, when_aiming, grip, weapon });
    }
    if let Some((owner, m)) = usp_model {
        let e = scene::spawn_model(&mut commands, &data.models[m].1, data, &mut cache, &mut meshes, &mut images, &mut materials, unlit, LevelEntity);
        commands.entity(e).insert(body::Held { owner, object: body::RIGHT_HAND, when_aiming: true, grip: body::pistol_grip(), weapon: Some(1) });
    }
}

/// The door Snake can open from where he stands, if any.
fn door_in_reach(game: &Game, snake: Vec3) -> Option<usize> {
    game.doors.iter().position(|d| {
        let (a, b, p) = (d.a.xz(), d.b.xz(), snake.xz());
        let ab = b - a;
        let t = if ab.length_squared() > 1.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        (a + ab * t).distance(p) < DOOR_REACH && (snake.y - d.a.y).abs() < 1000.0
    })
}

/// Space at a door runs its `-O` proc like the game does once the door is
/// open; a stage exit there sets Snake's next start and `load`s the next
/// stage, which replaces the current one.
#[allow(clippy::too_many_arguments)]
fn doors(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<Session>,
    mut game: ResMut<Game>,
    player: Query<(&Body, &Player)>,
    level: LevelRoots,
    bullets: Query<Entity, With<bullet::Bullet>>,
) {
    let Ok((body, p)) = player.single() else { return };
    if p.dead() {
        return;
    }
    let forced = match session.use_door {
        Some((d, at)) if time.elapsed_secs() >= at => {
            session.use_door = None;
            Some(d)
        }
        _ => None,
    };
    let door = match forced {
        Some(d) => Some(d).filter(|d| *d < game.doors.len()),
        None if keys.just_pressed(KeyCode::Space) => door_in_reach(&game, body.pos),
        None => None,
    };
    let Some(door) = door else { return };
    let proc = game.doors[door].on_open;
    sfx::play(sfx::DOOR, Some((game.doors[door].a + game.doors[door].b) * 0.5));
    info!("door {door}: opened");
    script_event(&mut commands, &session, &mut game, |vm| vm.call_proc(proc, &[0]), &level, &bullets);
}

type LevelRoots<'w, 's> = Query<'w, 's, Entity, (With<LevelEntity>, Without<ChildOf>)>;

/// Runs a script callback the way the game would at this moment (timers
/// and fades fire at once). Keeps the variables it sets; when it reaches
/// `load`, replaces the stage.
fn script_event(
    commands: &mut Commands,
    session: &Session,
    game: &mut Game,
    run: impl FnOnce(&mut gcl::Vm),
    level: &LevelRoots,
    bullets: &Query<Entity, With<bullet::Bullet>>,
) {
    if game.data.script.is_none() {
        return;
    }
    // Move the banks into the VM and back (this runs every frame for
    // traps that report "stay").
    let banks = std::mem::replace(&mut game.banks, gcl::Banks { a: Vec::new(), b: Vec::new() });
    let script = game.data.script.as_ref().unwrap();
    let mut vm = gcl::Vm::with_banks(script, banks);
    vm.eager = true;
    vm.timer = game.timer.as_ref().map_or(0, timer::Timer::frames);
    run(&mut vm);
    let (load, spawns, codec, banks) = vm.finish();
    if let Some(t) = spawns.iter().filter(|s| s.kind == timer::TIMER_ACTOR).find_map(timer::Timer::from_spawn) {
        game.timer = Some(t);
    }
    game.cameras.apply(&spawns);
    apply_codec(game, &codec);
    game.arrivals.extend(spawns.into_iter().filter(|s| s.kind == guard::TROOPER_ACTOR).map(|s| (s, TROOPER_DELAY)));
    let Some(mut next) = load else {
        game.banks = banks;
        return;
    };
    if next == gcl::GAME_OVER {
        info!("script: game over");
        game.game_over = true;
        game.banks = banks;
        return;
    }
    // `restart`: the same stage again, Snake placed by the variables.
    let restart = next == gcl::RESTART;
    if restart {
        next = game.data.name.clone();
    }
    let next = match next.strip_prefix('#') {
        // A hashed name: find the stage it stands for.
        Some(h) => {
            let h = u32::from_str_radix(h, 16).unwrap_or(0);
            let disc = session.disc.lock().unwrap();
            stage::tanker_stages(&disc).into_iter().find(|s| str_code(s.as_bytes()) == h).unwrap_or(next)
        }
        None => next,
    };
    let mut banks = banks;
    let (next, cutscene) = {
        let mut disc = session.disc.lock().unwrap();
        skip_cutscene(&mut disc, &session.iso, next, &game.data.name, &mut banks)
    };
    if let Some(mut c) = cutscene.filter(|_| !session.skip_cutscenes) {
        // The level the cutscene was reached from stays behind its actors.
        let mut disc = session.disc.lock().unwrap();
        c.backdrop = stage::load(&mut disc, &game.data.name).ok();
        commands.insert_resource(c);
    }
    if is_cutscene(&next) {
        // A cutscene that leads out of the Tanker (d12t, the chapter's end).
        info!("{next}: end of the Tanker chapter");
        commands.insert_resource(ChapterEnd);
        game.banks = banks;
        return;
    }
    info!("loading {next}");
    let level_result = {
        let mut disc = session.disc.lock().unwrap();
        prepare(&mut disc, &next, banks, !restart, session.print_spawns, session.show_vision)
    };
    let new = match level_result {
        Ok(l) => l,
        Err(e) => {
            error!("loading {next}: {e:#}");
            return;
        }
    };
    for e in level.iter().chain(bullets.iter()) {
        commands.entity(e).try_despawn();
    }
    *game = new.game;
    commands.insert_resource(new.phase);
    commands.insert_resource(NewCast(Mutex::new(Some(new.cast))));
}

/// Cutscene stages (`d..t`) are not played yet: follow the cutscene's own
/// exit to the next gameplay stage, with the start position and story
/// marker its script would set.
fn skip_cutscene(
    disc: &mut Disc,
    iso: &std::path::Path,
    name: String,
    leaving: &str,
    banks: &mut gcl::Banks,
) -> (String, Option<demo::Cutscene>) {
    if !is_cutscene(&name) {
        return (name, None);
    }
    let mut cutscene = None;
    let targets: Vec<String> = match stage::script(disc, &name) {
        Ok(Some(g)) => {
            let data = stage::load(disc, &name).ok();
            let scenery = data.as_ref().map(|d| cutscene_scenery(&g, d, banks)).unwrap_or_default();
            let mut c = demo::load(iso, &name, &g, data);
            c.scenery = scenery;
            cutscene = Some(c);
            let mut out = Vec::new();
            collect_loads(&g.main, &mut out);
            g.procs.iter().for_each(|p| collect_loads(&p.body, &mut out));
            out
        }
        _ => Vec::new(),
    };
    // Not back to the stage being left (a cutscene stage can sit between
    // two gameplay stages and load either).
    for target in targets.iter().filter(|t| t.starts_with('w') && t.as_str() != leaving) {
        if let Ok(arrivals) = stage::arrivals(disc, &name, target)
            && let Some(a) = arrivals.first() {
                set_snake_start(banks, a.entry.position, a.entry.direction);
                if let Some(p) = a.progress {
                    banks.set_a32(gcl::PROGRESS, p as i32);
                }
                info!("skipping cutscene {name} -> {target}");
                return (target.clone(), cutscene);
            }
    }
    (name, cutscene)
}

/// Cutscene stages are named `d..t`.
fn is_cutscene(name: &str) -> bool {
    name.starts_with('d') && name.ends_with('t')
}

/// The chapter is over (after its last cutscene).
#[derive(Resource)]
struct ChapterEnd;

#[derive(Component)]
struct EndScreen;

fn chapter_end(mut commands: Commands, end: Option<Res<ChapterEnd>>, shown: Query<(), With<EndScreen>>) {
    if end.is_none() || !shown.is_empty() {
        return;
    }
    commands.spawn((
        EndScreen,
        Text::new("TANKER CHAPTER COMPLETE\n\nThe Plant chapter is not part of this project."),
        TextFont { font_size: bevy::text::FontSize::Px(34.0), ..default() },
        TextColor(Color::srgb(0.9, 0.92, 0.85)),
        TextLayout::justify(Justify::Center),
        BackgroundColor(Color::BLACK),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::top(percent(20)),
            ..default()
        },
        GlobalZIndex(10),
    ));
}

/// Actors that place a cutscene's sky (`0x6273b5`) and sea (`0xa895c4`),
/// both `-m model -p x y z`.
const BACKDROP_ACTORS: &[u32] = &[0x6273B5, 0xA895C4];

/// The scenery a cutscene stage's script sets up: its props (the deck
/// pieces, `0x7a8ec1`), sky and sea, as (model, transform).
fn cutscene_scenery(script: &mgs2_formats::gcx::Gcx, data: &StageData, banks: &gcl::Banks) -> Vec<(usize, Transform, Option<u32>)> {
    let mut vm = gcl::Vm::with_banks(script, banks.clone());
    vm.run(&script.main);
    vm.spawns
        .iter()
        .filter(|s| PROP_ACTORS.contains(&s.kind) || BACKDROP_ACTORS.contains(&s.kind))
        .filter_map(|s| prop(s, data))
        .map(|p| (p.model, p.transform, p.lt2))
        .collect()
}

fn collect_loads(stmts: &[mgs2_formats::gcx::Stmt], out: &mut Vec<String>) {
    use mgs2_formats::gcx::{Stmt, Value};
    for s in stmts {
        if let Stmt::Command { name, args, options } = s {
            if *name == str_code(b"load")
                && let Some(Value::Str(t)) = args.first() {
                    out.push(t.clone());
                }
            for v in args.iter().chain(options.iter().flat_map(|(_, v)| v)) {
                if let Value::Block(b) = v {
                    collect_loads(b, out);
                }
            }
        }
    }
}

/// Olga beaten: after her fall, run her defeat callback, which ends the
/// fight the way the stage script does.
fn boss_defeat(
    mut commands: Commands,
    session: Res<Session>,
    mut game: ResMut<Game>,
    mut olgas: Query<&mut olga::Olga>,
    mut inv: ResMut<items::Inventory>,
    level: LevelRoots,
    bullets: Query<Entity, With<bullet::Bullet>>,
) {
    for mut o in &mut olgas {
        if o.down() && !o.reported && o.down_for > 3.0 {
            o.reported = true;
            // Snake takes her USP (rounds assumed).
            if !inv.usp {
                inv.usp = true;
                *inv.counts.entry(items::USP_AMMO).or_default() += items::USP_START;
                inv.message = Some(("USP".into(), 3.0));
            }
            if let Some(p) = o.on_down {
                info!("Olga is down: running {p:#x}");
                script_event(&mut commands, &session, &mut game, |vm| vm.call_proc(p, &[]), &level, &bullets);
                return;
            }
        }
    }
}

/// Codec requests from a script run: the area's call (on while its trap
/// reports enter/stay, off on leave), incoming calls, resets.
fn apply_codec(game: &mut Game, reqs: &[gcl::CodecReq]) {
    use gcl::{CodecReq, trap_event};
    for r in reqs {
        match *r {
            CodecReq::Area { event, freq, id } if event == trap_event::LEAVE => {
                if game.codec_area == Some((freq, id)) {
                    game.codec_area = None;
                }
            }
            CodecReq::Area { freq, id, .. } => game.codec_area = Some((freq, id)),
            CodecReq::Call { freq, id } => {
                if !game.codec_calls.iter().any(|c| (c.0, c.1) == (freq, id)) {
                    game.codec_calls.push((freq, id, None));
                }
            }
            CodecReq::Then { id, then } => game.codec_calls.push((14112, id, then)),
            CodecReq::Speech { id, then } => game.speech = Some((id, then, SPEECH_SECONDS)),
            CodecReq::Reset => game.codec_area = None,
            CodecReq::Sound(c) => game.sound_cmds.push(c),
            CodecReq::Timer(m) => {
                if let Some(t) = game.timer.as_mut() {
                    t.paused = m == 0;
                }
            }
        }
    }
}

/// The trap boxes Snake is in, with the game's enter / stay / leave
/// rules (`FUN_0014A448`): a handler is "in" while Snake is inside its box
/// and its conditions (`-b` button, `-d` facing) hold.
#[allow(clippy::too_many_arguments)]
fn traps(
    mut commands: Commands,
    time: Res<Time>,
    auto_press: Option<ResMut<AutoPress>>,
    keys: Res<ButtonInput<KeyCode>>,
    session: Res<Session>,
    mut game: ResMut<Game>,
    player: Query<(&Body, &Player)>,
    level: LevelRoots,
    bullets: Query<Entity, With<bullet::Bullet>>,
) {
    use gcl::trap_event::*;
    let Ok((body, p)) = player.single() else { return };
    if p.dead() {
        return;
    }
    let pos = body.pos + Vec3::Y * 10.0;
    let inside: Vec<u32> = game
        .data
        .collision
        .as_ref()
        .map(|h| {
            h.traps
                .iter()
                .filter(|t| (0..3).all(|k| t.min[k] <= pos[k] && pos[k] < t.max[k]))
                .map(|t| t.name & 0x00FF_FFFF)
                .collect()
        })
        .unwrap_or_default();
    let yaw = ((body.yaw / std::f32::consts::TAU * 4096.0).round() as i32) & 0xFFF;
    let now_s = time.elapsed_secs();
    let auto = auto_press.is_some_and(|mut a| {
        let hit = a.0.first().is_some_and(|t| *t <= now_s);
        if hit {
            a.0.remove(0);
        }
        hit
    });
    let pressed = keys.just_pressed(KeyCode::Space) || auto;
    let prime = std::mem::take(&mut game.prime_traps);
    let mut events = Vec::new();
    for (i, (t, was_in)) in game.traps.iter_mut().enumerate() {
        if t.actor != SNAKE_NAME {
            continue;
        }
        let in_box = inside.contains(&t.area);
        let facing = t.dir.is_none_or(|(d, r)| {
            let diff = (yaw - d).rem_euclid(4096);
            diff.min(4096 - diff) <= r
        });
        let now = in_box && facing && (!t.button || pressed);
        let event = match (*was_in, now) {
            (false, true) if t.mode != LEAVE => Some(ENTER),
            (true, true) if t.mode == STAY || t.mode == ALL => Some(STAY),
            (true, false) if matches!(t.mode, LEAVE | ENTER_LEAVE | ALL) => Some(LEAVE),
            _ => None,
        };
        *was_in = now;
        if prime {
            continue;
        }
        if let Some(e) = event {
            events.push((i, e));
        }
    }
    for (i, event) in events {
        let (t, _) = &game.traps[i];
        let (body_stmts, area) = (t.body.clone(), t.area);
        let args = [area as i32, SNAKE_NAME as i32, event as i32, pos.x as i32, pos.y as i32, pos.z as i32, 0];
        if session.trace_traps && event != STAY {
            eprintln!("trap {area:06x} event {event:06x}");
        }
        let name = game.data.name.clone();
        script_event(&mut commands, &session, &mut game, |vm| vm.run_block(&body_stmts, &args), &level, &bullets);
        if game.data.name != name {
            break; // a new stage replaced the traps
        }
    }
}

/// How long a spoken line (`#7aa13a`) lasts when its clip cannot be read.
const SPEECH_SECONDS: f32 = 8.0;

/// The subtitles of the spoken line playing, and its audio.
#[derive(Component)]
struct SpeechPart;

/// Plays spoken lines (voice and subtitles), ends them after their clip's
/// length and runs their procs.
#[allow(clippy::too_many_arguments)]
fn speech(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<Session>,
    mut game: ResMut<Game>,
    level: LevelRoots,
    bullets: Query<Entity, With<bullet::Bullet>>,
    codec_data: Option<Res<codec::CodecData>>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut text: Query<&mut Text, With<SpeechPart>>,
    parts: Query<Entity, With<SpeechPart>>,
    mut playing: Local<Option<(u32, Vec<mgs2_formats::demo::Line>, f32)>>,
) {
    let current = game.speech.map(|s| s.0);
    if playing.as_ref().map(|p| p.0) != current {
        for e in &parts {
            commands.entity(e).despawn();
        }
        *playing = None;
        if let (Some(id), Some(data)) = (current, codec_data.as_ref()) {
            match data.clip(id) {
                Ok((clip, lines)) => {
                    let secs = clip.frames as f32 / demo::TICKS;
                    let secs = if secs > 0.0 { secs } else { clip.samples.len() as f32 / f32::from(clip.channels) / clip.rate as f32 };
                    info!("speech {id:#x}: {secs:.1} s, {} lines", lines.len());
                    if let Some(s) = game.speech.as_mut() {
                        s.2 = secs;
                    }
                    commands.spawn((AudioPlayer(sources.add(AudioSource { bytes: clip.wav().into() })), PlaybackSettings::DESPAWN, SpeechPart));
                    commands.spawn((
                        SpeechPart,
                        Text::new(""),
                        TextFont { font_size: bevy::text::FontSize::Px(22.0), ..default() },
                        TextColor(Color::srgb(0.95, 0.95, 0.9)),
                        TextLayout::justify(Justify::Center),
                        Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: px(48), ..default() },
                        GlobalZIndex(4),
                    ));
                    *playing = Some((id, lines, secs));
                }
                Err(e) => {
                    warn!("speech {id:#x}: {e:#}");
                    *playing = Some((id, Vec::new(), SPEECH_SECONDS));
                }
            }
        }
    }
    let Some((id, then, left)) = game.speech.as_mut() else { return };
    *left -= time.delta_secs();
    if let Some((_, lines, total)) = playing.as_ref() {
        let t = (total - *left) * demo::TICKS;
        let line = lines.iter().rev().find(|l| l.start as f32 <= t && t <= l.end as f32);
        for mut tx in &mut text {
            let want = line.map_or("", |l| l.text.as_str());
            if tx.0 != want {
                tx.0 = want.to_string();
            }
        }
    }
    if *left > 0.0 {
        return;
    }
    let (id, then) = (*id, *then);
    game.speech = None;
    if let Some(p) = then {
        info!("speech {id:#x} over: running {p:#x}");
        script_event(&mut commands, &session, &mut game, |vm| vm.call_proc(p, &[0]), &level, &bullets);
    }
}

/// Counts the holds' timer down and runs its proc when it runs out.
fn holds_timer(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<Session>,
    mut game: ResMut<Game>,
    level: LevelRoots,
    bullets: Query<Entity, With<bullet::Bullet>>,
) {
    let Some(t) = game.bypass_change_detection().timer.as_mut() else { return };
    if let Some(p) = t.tick(time.delta_secs()) {
        info!("holds timer ran out: running {p:#x}");
        script_event(&mut commands, &session, &mut game, |vm| vm.call_proc(p, &[0]), &level, &bullets);
    }
}

/// Guards' near sight range by difficulty (As16:0x1bc, set by the title
/// screen's script; 6000 if unset).
fn banks_vision(banks: &gcl::Banks) -> f32 {
    let v = i16::from_le_bytes([banks.a[0x1BC], banks.a[0x1BD]]);
    if v > 0 { f32::from(v) } else { 6000.0 }
}

/// Seconds before troopers arrive after the alert (the squad's `-w`, 180
/// frames in w00a; default 0xf0).
const TROOPER_DELAY: f32 = 3.0;

/// A reinforcement trooper and where he came in.
#[derive(Component)]
struct Trooper {
    entry: Vec3,
}

/// Reinforcements: the manager's `-a` proc when an alert starts (once
/// while troopers are still around), troopers arriving after their delay,
/// and leaving through their entry once the alert is over.
#[allow(clippy::too_many_arguments)]
fn reinforcements(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<Session>,
    phase: Res<Phase>,
    mut game: ResMut<Game>,
    mut last: Local<guard::PhaseKind>,
    troopers: Query<(Entity, &Trooper, &Guard, &Body)>,
    level: LevelRoots,
    bullets: Query<Entity, With<bullet::Bullet>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let entered = *last != guard::PhaseKind::Alert && phase.kind == guard::PhaseKind::Alert;
    *last = phase.kind;
    if entered && troopers.is_empty() && game.arrivals.is_empty()
        && let Some(p) = game.on_alert
    {
        info!("alert: running {p:#x} (reinforcements)");
        script_event(&mut commands, &session, &mut game, |vm| vm.call_proc(p, &[]), &level, &bullets);
    }
    // Leaving once the alert and evasion are over.
    if matches!(phase.kind, guard::PhaseKind::Caution | guard::PhaseKind::Infiltration) {
        for (e, t, g, b) in &troopers {
            if g.idle_at_start() && b.pos.distance(t.entry) < 800.0 {
                commands.entity(e).despawn();
            }
        }
    }
    let dt = time.delta_secs();
    let mut due = Vec::new();
    game.arrivals.retain_mut(|(s, left)| {
        *left -= dt;
        if *left <= 0.0 {
            due.push(s.clone());
        }
        *left > 0.0
    });
    if due.is_empty() {
        return;
    }
    let data = &game.data;
    let (Some(model), motions) = (model_index(data, "gbs_def"), motions_index(data, "gbs")) else { return };
    let gun = model_index(data, "aks");
    let mut cache = MaterialCache { cull: session.cull, ..default() };
    for s in due {
        let Some((mut g, mut pos)) = guard::from_spawn(&s, 6000.0) else { continue };
        if s.opt(b'd').is_none() {
            let Some(start) = guard::route_start(&game, g.route) else { continue };
            pos = start;
        }
        if let Some(y) = game.collision.as_ref().and_then(|c| c.floor_below(pos.x, pos.z, pos.y + 1000.0)) {
            pos.y = y;
        }
        if phase.kind == guard::PhaseKind::Alert {
            g.join_alert(phase.last_seen);
        }
        info!("trooper arrives at {pos} (route {})", g.route);
        let e = body::spawn_body(
            &mut commands,
            &data.models[model].1,
            data,
            &mut cache,
            &mut meshes,
            &mut images,
            &mut materials,
            true,
            (Body::new(pos, 0.0, model, motions), LevelEntity),
        );
        commands.entity(e).insert((g, Trooper { entry: pos }));
        if let Some(m) = gun {
            let w = scene::spawn_model(&mut commands, &data.models[m].1, data, &mut cache, &mut meshes, &mut images, &mut materials, true, LevelEntity);
            commands.entity(w).insert(body::Held { owner: e, object: body::RIGHT_HAND, when_aiming: false, grip: body::rifle_grip(), weapon: None });
        }
    }
}

/// Snake's position as the scripts read it (Bu32 0xe0 / 0xe4 / 0xe8; the
/// height is taken as his body's centre: the holds' scripts test it
/// against the floor level -17000 and the catwalks' -9000), and a game
/// over the script asked for.
fn snake_vars(mut game: ResMut<Game>, mut player: Query<(&Body, &mut Player)>) {
    let Ok((b, mut p)) = player.single_mut() else { return };
    let centre = b.pos.y + p.height() * 0.5;
    for (i, v) in [b.pos.x, centre, b.pos.z].into_iter().enumerate() {
        let o = 0xE0 + i * 4;
        game.banks.b[o..o + 4].copy_from_slice(&(v as i32).to_le_bytes());
    }
    if std::mem::take(&mut game.game_over) && !p.dead() {
        p.life = 0;
    }
}

/// The Marines in formation watching: one who sees Snake (inside `-t
/// near`, or inside `far` while Snake stands, within ±0x200 of his
/// facing, with a clear line) raises the crowd's "saw Snake" event with
/// his position, once (the game clears the proc after calling it).
fn crowd_watch(
    mut commands: Commands,
    session: Res<Session>,
    mut game: ResMut<Game>,
    marines: Query<&Body, With<crowd::Marine>>,
    player: Query<(&Body, &Player), Without<crowd::Marine>>,
    level: LevelRoots,
    bullets: Query<Entity, With<bullet::Bullet>>,
) {
    if game.crowd_alarm {
        return;
    }
    let Some(Some(proc)) = game.crowd_procs.get(crowd::SAW_SNAKE).copied() else { return };
    let Ok((snake, p)) = player.single() else { return };
    if p.dead() {
        return;
    }
    let (near, far) = game.crowd_sight;
    let target = snake.pos + Vec3::Y * (p.height() * 0.6);
    let seen = marines.iter().find(|m| {
        let to = snake.pos - m.pos;
        let dist = to.length();
        if dist > far || (dist > near && (p.crouch || p.prone)) {
            return false;
        }
        let ang = (to.x.atan2(to.z) - m.yaw).rem_euclid(std::f32::consts::TAU);
        let ang = ang.min(std::f32::consts::TAU - ang);
        let eye = m.pos + Vec3::Y * 1600.0;
        ang <= std::f32::consts::FRAC_PI_4 && !game.collision.as_ref().is_some_and(|c| c.line_blocked(eye, target))
    });
    let Some(m) = seen else { return };
    let at = [m.pos.x as i32, m.pos.y as i32, m.pos.z as i32];
    game.crowd_alarm = true;
    info!("a Marine saw Snake at {}: running {proc:#x}", snake.pos);
    script_event(&mut commands, &session, &mut game, |vm| vm.call_proc(proc, &at), &level, &bullets);
}

/// `--press-at`: action button presses at these times (seconds).
#[derive(Resource)]
struct AutoPress(Vec<f32>);

/// Runs the procs of Codec calls that have ended (`#db03a6 ... -p`).
fn codec_followup(
    mut commands: Commands,
    session: Res<Session>,
    mut game: ResMut<Game>,
    level: LevelRoots,
    bullets: Query<Entity, With<bullet::Bullet>>,
) {
    if let Some(p) = game.codec_then.pop() {
        info!("codec call over: running {p:#x}");
        script_event(&mut commands, &session, &mut game, |vm| vm.call_proc(p, &[]), &level, &bullets);
    }
}

fn toggles(keys: Res<ButtonInput<KeyCode>>, mut game: ResMut<Game>) {
    if keys.just_pressed(KeyCode::F1) {
        game.show_collision = !game.show_collision;
    }
    if keys.just_pressed(KeyCode::F2) {
        game.show_vision = !game.show_vision;
    }
}

fn draw_collision(game: Res<Game>, mut gizmos: Gizmos) {
    for d in &game.doors {
        gizmos.line(d.a * SCALE + Vec3::Y, d.b * SCALE + Vec3::Y, Color::srgb(0.2, 1.0, 1.0));
    }
    if !game.show_collision {
        return;
    }
    let Some(h) = &game.data.collision else { return };
    let p = |a: [f32; 3]| Vec3::from(a) * SCALE;
    for g in h.areas.iter().flat_map(|a| &a.groups) {
        for w in &g.walls {
            for pair in w.points.windows(2) {
                let ((a, ha), (b, hb)) = (pair[0], pair[1]);
                let (a, b) = (p(a), p(b));
                gizmos.line(a, b, Color::srgb(1.0, 0.2, 0.15));
                gizmos.line(a + Vec3::Y * ha * SCALE, b + Vec3::Y * hb * SCALE, Color::srgb(1.0, 0.2, 0.15));
            }
        }
        for poly in g.floors.iter().flat_map(|f| &f.polygons) {
            let n = poly.vertices.len();
            for i in 0..n {
                gizmos.line(p(poly.vertices[i]), p(poly.vertices[(i + 1) % n]), Color::srgb(0.2, 1.0, 0.3));
            }
        }
    }
    for r in &h.routes {
        for pair in r.points.windows(2) {
            gizmos.line(p(pair[0].position), p(pair[1].position), Color::srgb(0.9, 0.5, 1.0));
        }
    }
}

/// After a game over, Enter continues from the stage start with full life
/// and the guards back on patrol.
#[allow(clippy::type_complexity)]
fn game_over(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    session: Res<Session>,
    mut game: ResMut<Game>,
    mut phase: ResMut<Phase>,
    mut player: Query<(&mut Player, &mut Body), Without<Guard>>,
    mut guards: Query<&mut Guard>,
    troopers: Query<Entity, With<Trooper>>,
    bullets: Query<Entity, With<bullet::Bullet>>,
) {
    let Ok((mut p, mut body)) = player.single_mut() else { return };
    if !p.dead() || !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    *p = Player::new(session.difficulty);
    (body.pos, body.yaw) = game.snake_start;
    let caution_time = phase.caution_time;
    *phase = Phase::default();
    phase.caution_time = caution_time;
    for mut g in &mut guards {
        g.reset();
    }
    for e in bullets.iter().chain(troopers.iter()) {
        commands.entity(e).despawn();
    }
    game.arrivals.clear();
    game.banks = game.start_banks.clone();
    game.crowd_alarm = false;
}

fn hud(
    game: Res<Game>,
    phase: Res<Phase>,
    player: Query<(&Body, &Player)>,
    guards: Query<&Guard>,
    inv: Res<items::Inventory>,
    olgas: Query<&olga::Olga>,
    mut text: Query<&mut Text, With<Hud>>,
    time: Res<Time>,
    mut logged: Local<u32>,
) {
    let (Ok((b, p)), Ok(mut t)) = (player.single(), text.single_mut()) else { return };
    // MGS2_POS_LOG=1: Snake's position twice a second (testing).
    if std::env::var_os("MGS2_POS_LOG").is_some() {
        let n = (time.elapsed_secs() * 2.0) as u32;
        if *logged != n {
            *logged = n;
            info!("t {:.1} snake {:.0} {:.0} {:.0} motion {}", time.elapsed_secs(), b.pos.x, b.pos.y, b.pos.z, b.motion);
        }
    }
    let status = match phase.kind {
        guard::PhaseKind::Alert => format!("ALERT  (meter {})", phase.meter),
        guard::PhaseKind::Evasion => format!("EVASION {:.1}", phase.evasion as f32 / 60.0),
        guard::PhaseKind::Caution => format!("CAUTION {:.1}", phase.caution as f32 / 60.0),
        guard::PhaseKind::Infiltration => "INFILTRATION".into(),
    };
    let bar = |n: i32, max: i32| {
        let full = (n.max(0) * 20 + max - 1) / max.max(1);
        format!("[{}{}]", "#".repeat(full as usize), "-".repeat(20 - full as usize))
    };
    let life = if p.dead() {
        "LIFE [--------------------]  GAME OVER - press Enter to continue".to_string()
    } else {
        let grip = p.hang.map_or(String::new(), |h| format!("   GRIP {}", bar((h.grip * 100.0) as i32, 100)));
        format!("LIFE {} {}/{}{}{grip}", bar(p.life, p.max_life), p.life, p.max_life, if p.hurt > 0.0 { "  HIT!" } else { "" })
    };
    let mut prompt = if door_in_reach(&game, b.pos).is_some() { "  [Space] open door".to_string() } else { String::new() };
    for o in &olgas {
        prompt += &if o.down() { "   OLGA: down".to_string() } else { format!("   OLGA {}", bar(o.stamina, olga::STAMINA)) };
    }
    let mut carried: Vec<String> =
        inv.counts.iter().filter(|(_, n)| **n > 0).map(|((k, i), n)| format!("{} {n}", items::name(*k, *i))).collect();
    if inv.ration_equipped {
        carried.insert(0, "[Ration equipped]".into());
    }
    if inv.usp {
        carried.insert(0, format!("[{}]", if inv.weapon == 1 { "USP" } else { "M9" }));
    }
    let mut message = inv.message.as_ref().map_or(String::new(), |(m, _)| format!("  Got: {m}"));
    if let Some(t) = &game.timer {
        message += &format!("   TIME {}", t.text());
    }
    **t = format!(
        "{}  {}  guards {}  camera: {}\n{}{}\nitems: {}{}\npos {:.0} {:.0} {:.0} mm  motion {}{}\n\
         WASD move  Shift walk  C crouch  Z prone  Space action  F aim/fire  1/2 M9/USP  V punch  R equip ration  X first person  B box  G grenade  Q/E or RMB camera  F1 collision+routes  F2 vision cones  F3 script cameras  F4 hide this",
        game.data.name,
        status,
        guards.iter().count(),
        match &game.cameras.active { Some((n, _)) => format!("stage {n:06x}"), None => "follow".into() },
        life,
        prompt,
        if carried.is_empty() { "-".to_string() } else { carried.join(", ") },
        message,
        b.pos.x,
        b.pos.y,
        b.pos.z,
        b.motion,
        if p.crouch { "  (crouching)" } else { "" },
    );
}

fn capture(
    mut commands: Commands,
    time: Res<Time>,
    real: Res<Time<Real>>,
    paused: Res<pause::Paused>,
    mut held: Local<f32>,
    mut c: ResMut<Capture>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    // Game time, plus real time spent paused (so a paused test still ends).
    if paused.0 {
        *held += real.delta_secs();
    }
    let t = time.elapsed_secs() + *held;
    if !c.taken && t > c.at {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(c.path.clone()));
        c.taken = true;
    }
    if t > c.at + 2.0 {
        exit.write(AppExit::Success);
    }
}

/// `MGS2_GUARD_SHEET=<dir>` with `MGS2_SHEET=motion:frame,...`: from 3 s
/// on, puts the nearest guard 1.5 m to Snake's right on screen and holds him in each
/// listed pose in turn (half a second each), saving `<dir>/<motion>_<frame>.png`.
fn guard_sheet(
    mut commands: Commands,
    time: Res<Time>,
    players: Query<&body::Body, (With<player::Player>, Without<guard::Guard>)>,
    mut guards: Query<&mut body::Body, With<guard::Guard>>,
    cams: Query<&Transform, With<player::FollowCam>>,
    mut last: Local<Option<usize>>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let (Some(dir), Some(list)) = (std::env::var_os("MGS2_GUARD_SHEET"), std::env::var("MGS2_SHEET").ok()) else { return };
    let poses: Vec<(usize, f32)> = list
        .split(',')
        .filter_map(|p| p.split_once(':').and_then(|(m, f)| Some((m.trim().parse().ok()?, f.trim().parse().ok()?))))
        .collect();
    let t = time.elapsed_secs() - 3.0;
    let Ok(snake) = players.single() else { return };
    if t < 0.0 {
        return;
    }
    let n = (t / 0.5) as usize;
    let Some(&(motion, frame)) = poses.get(n) else {
        exit.write(AppExit::Success);
        return;
    };
    // Beside Snake, across the camera's view.
    let flat = Vec3::new(1.0, 0.0, 1.0);
    let (right, back) = cams.single().map_or((Vec3::X, Vec3::Z), |c| ((c.right().as_vec3() * flat).normalize_or(Vec3::X), (c.back().as_vec3() * flat).normalize_or(Vec3::Z)));
    let Some(mut b) = guards.iter_mut().min_by(|a, c| a.pos.distance(snake.pos).total_cmp(&c.pos.distance(snake.pos))) else { return };
    b.pos = snake.pos + right * 1200.0;
    // Facing the camera.
    b.yaw = back.x.atan2(back.z);
    b.motion = motion;
    b.once = true;
    b.frame = frame;
    if (t % 0.5) > 0.35 && *last != Some(n) {
        *last = Some(n);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(std::path::Path::new(&dir).join(format!("{motion:03}_{frame:03}.png"))));
    }
}

/// `MGS2_MOTION_SHEET=<dir>`: from 3 s on, holds Snake in each of his
/// motions in turn (its middle frame, half a second each) and saves a
/// screenshot of each as `<dir>/<n>.png`, then exits (to identify motions).
fn motion_sheet(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut q: Query<&mut body::Body, With<player::Player>>,
    mut last: Local<Option<usize>>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    // MGS2_MOTION_LENGTHS=1: log each motion archive's lengths once.
    if std::env::var_os("MGS2_MOTION_LENGTHS").is_some() && last.is_none() {
        *last = Some(usize::MAX);
        for (name, set) in &game.data.motions {
            let l: Vec<String> = set.motions.iter().enumerate().map(|(i, m)| format!("{i}:{:#x}:{}", m.id, m.length)).collect();
            info!("motions {name}: {}", l.join(" "));
        }
    }
    let Some(dir) = std::env::var_os("MGS2_MOTION_SHEET") else { return };
    let t = time.elapsed_secs() - 3.0;
    let Ok(mut b) = q.single_mut() else { return };
    let Some(set) = b.motions.map(|m| &game.data.motions[m].1) else { return };
    if t < 0.0 {
        return;
    }
    let n = (t / 0.5) as usize;
    if n >= set.motions.len() {
        exit.write(AppExit::Success);
        return;
    }
    b.motion = n;
    b.once = true;
    b.frame = (set.motions[n].length / 2) as f32;
    if (t % 0.5) > 0.35 && *last != Some(n) {
        *last = Some(n);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(std::path::Path::new(&dir).join(format!("{n:03}.png"))));
    }
}