//! A GCL interpreter for stage scripts, following the game's VM
//! (`FUN_00153010` blocks, `FUN_00154080` expressions, `FUN_00153628`
//! variable access). Commands that create actors are recorded as [`Spawn`]s
//! for the game to instantiate; commands not implemented yet are skipped.

use std::collections::HashMap;

use mgs2_formats::gcx::{ExprItem, Gcx, Op, Stmt, Value, Var};
use mgs2_formats::str_code;

/// An evaluated argument.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Int(i32),
    Name(u32),
    Proc(u32),
    Str(String),
}

impl Val {
    pub fn int(&self) -> Option<i32> {
        match self {
            Val::Int(i) => Some(*i),
            Val::Name(h) | Val::Proc(h) => Some(*h as i32),
            Val::Str(_) => None,
        }
    }
}

/// `chara <kind> <args...> -x <values...>` with everything evaluated.
#[derive(Clone, Debug)]
pub struct Spawn {
    pub kind: u32,
    pub args: Vec<Val>,
    pub options: Vec<(u8, Vec<Val>)>,
}

impl Spawn {
    pub fn opt(&self, letter: u8) -> Option<&[Val]> {
        self.options.iter().find(|(l, _)| *l == letter).map(|(_, v)| v.as_slice())
    }

    pub fn opt_int(&self, letter: u8, i: usize) -> Option<i32> {
        self.opt(letter)?.get(i)?.int()
    }
}

/// `trap <box> <actor> -m <mode> [-b n] [-d dir range] -e { ... }`: a
/// handler for an `.hzx` trap box (`FUN_0014A448` dispatches to it).
#[derive(Clone, Debug)]
pub struct Trap {
    /// Trap box name.
    pub area: u32,
    /// Name of the actor it reacts to (Snake is 0x28dce6).
    pub actor: u32,
    /// Which events run the block, see [`trap_event`].
    pub mode: u32,
    /// `-b`: only while a button is pressed.
    pub button: bool,
    /// `-d`: only while the actor faces `dir` within `range` (4096 units).
    pub dir: Option<(i32, i32)>,
    pub body: Vec<Stmt>,
}

/// Trap events (the block's `$3`).
pub mod trap_event {
    pub const ENTER: u32 = 0x670D6B;
    pub const STAY: u32 = 0x54A56B;
    pub const LEAVE: u32 = 0x61D56B;
    /// Modes: enter and leave / enter, stay and leave.
    pub const ENTER_LEAVE: u32 = 0x14C9;
    pub const ALL: u32 = 0x1516;
}

/// Actors that call one of their proc options as soon as they are created,
/// as far as stage setup needs: `0xddf5ca` (enemy manager, `FUN_002181f0`:
/// every `-w` proc, then every `-a` proc, through `FUN_0021a688`; `-w`
/// creates the "wccomm" actor, `-a` a reinforcement squad or w03a / w03b's
/// "atcomm"), `0x59eda8` "wccomm" (`-e` creates the guards) and
/// `0x5bb4d4` (a reinforcement squad, `FUN_0029dcb8`: `-e` creates its
/// troopers), `0xc7280a` "atcomm" (w03a / w03b, `atcomm.c` `FUN_00272b48` ->
/// `FUN_00272a78`: `-e` creates the Deck-2 defenders).
const CALL_ON_CREATE: &[(u32, u8)] = &[(0xDDF5CA, b'w'), (0xDDF5CA, b'a'), (0x59EDA8, b'e'), (0x5BB4D4, b'e'), (0xC7280A, b'e')];
/// Difficulty (bank B s16): 10 very easy ... 50 extreme.
pub const DIFFICULTY: u16 = 0x8;
/// Story marker (bank A u32) set by the stage that sent Snake here.
pub const PROGRESS: u16 = 0xC4;
/// Bu32:0xb4: the current stage's name hash (set by the engine; the holds'
/// shared script branches on it).
pub const STAGE_NAME: u16 = 0xB4;

/// Snake's position and facing for the next stage (bank A u32), as set by
/// a stage exit before `load` and read by the player actor's `-p`/`-d`.
pub const SNAKE_X: u16 = 0xBC;
pub const SNAKE_Y: u16 = 0x7F4;
pub const SNAKE_Z: u16 = 0xC0;
pub const SNAKE_DIR: u16 = 0x7F8;

/// Actors that run their `-p` callback right away when the VM runs an
/// event (`eager`): the fade-out `0xe22388` and `delay` timers
/// (`0x743c9f`), so a door's exit proc reaches its `load` and the holds'
/// "caught" sequence its game over. (A `delay`'s `-e` block always runs at
/// once.)
const EAGER: &[u32] = &[0xE22388, DELAY_ACTOR];
/// The `delay` timer actor (`cmd_743c9f`).
const DELAY_ACTOR: u32 = 0x743C9F;

/// Commands that run their `-p` proc once their action is over; with
/// `eager` it runs at once. `0x709df6`: Snake plays an action (`-m`) and
/// then calls back (the holds' terminal).
const CALL_AFTER: &[u32] = &[0x709DF6];
/// `command #c74f97 <var> <values...>`: stores the values in consecutive
/// elements of an array variable.
const FILL_ARRAY: u32 = 0xC74F97;
/// `command #661bee -p <4 corners> -f <fill %> -R <proc> [-x n] [-y n]`: a
/// target for the digital camera (w04c resident `cmd_661bee`).
const PHOTO_TARGET: u32 = 0x661BEE;
/// `command #8b12d1 -p <proc>`: called after each photo has been graded.
const PHOTO_DONE: u32 = 0x8B12D1;
/// A Codec call that runs a proc when it ends: `#db03a6 -c <id> -p <proc>`
/// (sending photos from the holds' terminal).
const CODEC_THEN: &[u32] = &[0xDB03A6];
/// `#7aa13a <id> -p <proc>`: a line of spoken dialogue outside the Codec
/// (ids 0x2000xxxx; the holds chain the commandant's speech this way, each
/// line's proc picking the next). The proc gets 0 when the line is over.
const SPEECH: u32 = 0x7AA13A;
/// `#339bd9 <handle> -v <volume 0-63> -p <pan 0-63>`: the volume of a line
/// started by `#7aa13a` (the holds fade the commandant's speech with
/// Snake's distance); the pan is not applied.
const VOLUME: u32 = 0x339BD9;
/// `#b44bbb [-c] -i <table> -t <which> -s <sound> -d <id byte> x 8`
/// (`cmd_b44bbb`): the eight variants of a motion sound by floor material
/// for one character table; `-c` clears them.
const SOUND_TABLE: u32 = 0xB44BBB;
/// `#3822c7 <actor> <code> args..`: a message to an actor.
const SEND_MESSAGE: u32 = 0x3822C7;
/// `#c1eb06 <actor> x y z` (`cmd_c1eb06`): writes the actor's position to
/// the three variables; 1, or -1 if it is not found.
const ACTOR_POS: u32 = 0xC1EB06;
/// `#41cf4e -n <material>` (`cmd_41cf4e`): rounds striking that material
/// sound its variant of motion sound 0x27 and throw a spark burst
/// (`FUN_003a5cf8` -> `FUN_003057f8`); bullets look
/// the hook up by the struck element's flags >> 28 (`FUN_0036c630`).
const IMPACT_SOUND: u32 = 0x41CF4E;
/// `#0acf09 -n <material>` (`cmd_0acf09`): rounds striking that material
/// leave a bullet hole (`FUN_002d7498`, `wall_scar.c`'s own placement).
const IMPACT_SCARS: u32 = 0x0ACF09;
/// `command #f6ada8` (w03a / w03b's proc `#c4bdfe`; `command` calls a
/// resident function by name, `FUN_0010a848`): sets DAT_00191a14, which
/// lets the weapon manager (`FUN_0033ca68`) light the USP while Snake aims
/// it. Cleared at each stage's start (`FUN_00154218`).
const WEAPON_LIGHT: u32 = 0xF6ADA8;
/// `#542b2d -a <n> -r <times> -d <values..> -e {block}` (`cmd_542b2d`):
/// runs the block `times` times, each with the next `n` values as `$1..`.
const REPEAT: u32 = 0x542B2D;
/// `#f2eedc -c <command>`: a raw sound driver command (sound effects
/// when the high byte is 0).
const SOUND: u32 = 0xF2EEDC;
/// The sound relay actor's name (`mesg #9b5b7f <target> <command>`).
const SOUND_RELAY: u32 = 0x9B5B7F;
/// `#89a17e { ... }` in Codec records: repeats its block while the block
/// returns non-zero.
const CODEC_LOOP: u32 = 0x89A17E;

/// A photo target: the camera grades how a photo shows this quad and calls
/// `proc` with the grade (`FUN_0031cc18`, see `photo.rs`).
#[derive(Clone, Debug)]
pub struct PhotoTarget {
    /// Corners (mm) in order; the normal is `(c2 - c1) x (c1 - c0)`.
    pub corners: [[f32; 3]; 4],
    /// `-f`: share of the screen (percent) the quad must cover.
    pub fill: f32,
    /// `-R`: called with the grade 0..4.
    pub proc: u32,
    /// `-x` / `-y`: how far (4096ths) the view may be from the quad's
    /// normal in pitch / yaw.
    pub pitch: i32,
    pub yaw: i32,
}

/// The two variable banks, kept across stages like the game does.
#[derive(Clone)]
pub struct Banks {
    pub a: Vec<u8>,
    pub b: Vec<u8>,
}

impl Default for Banks {
    fn default() -> Self {
        Self { a: vec![0; 0x10000], b: vec![0; 0x10000] }
    }
}

pub struct Vm<'a> {
    procs: HashMap<u32, &'a [Stmt]>,
    /// Bank at 0x191918 (`A` in disassembly) and 0x191910 (`B`).
    bank_a: Vec<u8>,
    bank_b: Vec<u8>,
    pub spawns: Vec<Spawn>,
    /// Where actors the port does not run stand (mm), for `#c1eb06`: the
    /// holds' speakers from the crowd's `-c`.
    pub places: HashMap<u32, [i32; 3]>,
    pub traps: Vec<Trap>,
    /// Stage named by the first `load` reached.
    pub load: Option<String>,
    /// Run timer/fade callbacks immediately and stop at `load`.
    pub eager: bool,
    /// Frames left on the holds' timer (`command #450c64`).
    pub timer: i32,
    /// The song last requested of the sound driver (`FUN_00112b88`: a
    /// command 0x01000001..8 sets it, 0x0200000a..d clears it), as
    /// `command #d9c789` reports it.
    pub song: i32,
    /// Codec requests from the stage script.
    pub codec: Vec<CodecReq>,
    /// Running a CODEC.DAT record: collect its dialogue lines.
    pub codec_mode: bool,
    /// (speaker name hash, text) in order.
    pub lines: Vec<(u32, String)>,
    /// The voice clip (VOX.DAT id) each line belongs to.
    pub line_voices: Vec<u32>,
    voice: u32,
    /// Digital camera targets and the proc run after each photo.
    pub photos: Vec<PhotoTarget>,
    pub photo_done: Option<u32>,
    strings: &'a [String],
    last: i32,
    depth: u32,
    rng: u32,
}

/// What `load` holds after `restart`: the current stage again.
pub const RESTART: &str = "<restart>";
/// What `load` holds after `load <stage> -g`: game over.
pub const GAME_OVER: &str = "<game over>";
/// `command #635875 [-l] -e { ... }`: runs the block once nothing else is
/// on screen (the holds' "caught" and "time up" sequences); run at once.
const WHEN_FREE: u32 = 0x635875;
/// `command #450c64`: frames left on the holds' timer.
const TIMER_LEFT: u32 = 0x450C64;

/// The Codec command (`command #4a243a ...`) in stage scripts.
pub const CODEC_COMMAND: u32 = 0x4A243A;
/// `cmd_d9c789`: the song last requested ([`Vm::song`]).
const SONG: u32 = 0xD9C789;
/// Script maths: sin, cos (4096 = a full turn, results times 4096) and abs.
const SIN: u32 = 0x01D98E;
const COS: u32 = 0x019A53;
const ABS: u32 = 0x0190B3;
/// `cmd_82bdc0 x1 y1 z1 x2 y2 z2`: the distance between two points.
const DISTANCE: u32 = 0x82BDC0;
/// `cmd_325c8e a b`: `atan2f(b, a)` as a 4096-unit angle.
const ATAN: u32 = 0x325C8E;
/// Snake's life (Bs16:0xf2; the maximum is Bs16:0xf4).
pub const SNAKE_LIFE: u16 = 0xF2;
/// `command #ee8b90 -s effect -p x y z`: a sound effect at a place.
pub const SOUND_AT_COMMAND: u32 = 0xEE8B90;
/// `command #80b977 -g proc -c proc`: the stage's game-over proc and the
/// proc run on continuing.
pub const OVER_COMMAND: u32 = 0x80B977;
/// `cmd_5a4652 -v id`: the voice clip for the game-over screen.
pub const DEATH_VOICE_COMMAND: u32 = 0x5A4652;
/// `cmd_944e11`: game features off / on (`-m "off" -r "on"` ...).
pub const FLAGS_COMMAND: u32 = 0x944E11;

/// The feature bits `cmd_944e11` sets (DAT_0017f498; `PAUSE` is DAT_0017f490
/// 0x200, kept here as a high bit).
pub mod flags {
    /// `-m`: the weapon (1) and item (2) windows.
    pub const MENUS: u32 = 3;
    /// `-r`: the radar (`FUN_00393318` hides it).
    pub const RADAR: u32 = 4;
    /// `-g`.
    pub const GUN: u32 = 8;
    /// `-s`.
    pub const S: u32 = 0x20;
    /// `-A`.
    pub const ACTION: u32 = 0x40000;
    /// `-p`: pausing.
    pub const PAUSE: u32 = 0x8000_0000;
}
/// A line of Codec dialogue.
const CODEC_LINE: u32 = 0x3B91EB;
/// `#01e658 <voice id> { lines }`: the lines spoken in one VOX.DAT clip.
const CODEC_VOICE: u32 = 0x01E658;

#[derive(Clone, Debug)]
pub enum CodecReq {
    /// The conversation Snake gets when he calls `freq` in this area
    /// (the trap event decides on/off).
    Area { event: u32, freq: i32, id: u32 },
    /// The stage's default conversation on `freq` (`-b`), for anywhere
    /// without an area call.
    Base { freq: i32, id: u32 },
    /// An incoming call.
    Call { freq: i32, id: u32 },
    /// A call that runs `then` when it ends.
    Then { id: u32, then: Option<u32> },
    /// Spoken dialogue that runs `then` when it ends.
    Speech { id: u32, then: Option<u32> },
    /// A motion sound's variants by floor material (`#b44bbb`); `None`
    /// clears them all.
    SoundTable(Option<SoundTable>),
    /// Rounds striking this material make its impact sound (`#41cf4e`).
    ImpactSound(u8),
    /// Rounds striking this material leave bullet holes (`#0acf09`).
    ImpactScars(u8),
    /// The USP's light is allowed in this stage (`#f6ada8`).
    WeaponLight,
    /// The volume (0-63) of the spoken line `handle`.
    Volume { handle: u32, vol: u8 },
    Reset,
    /// Features switched on and off (`cmd_944e11`, [`flags`]).
    Flags { on: u32, off: u32 },
    /// A sound effect at a place (`#ee8b90`).
    SoundAt { id: u32, at: [i32; 3] },
    /// A message to an actor (`mesg <actor> <code> args..`).
    Mesg { target: u32, code: u32, args: Vec<i32> },
    /// The game-over and continue procs (`#80b977 -g -c`).
    OverProcs { over: Option<u32>, cont: Option<u32> },
    /// The voice for the game-over screen (`#5a4652 -v`).
    DeathVoice(u32),
    /// A raw sound driver command (`#f2eedc -c`).
    Sound(u32),
    /// A message to the holds' timer: 0 pause, 1 run.
    Timer(u32),
}

/// One `#b44bbb` entry: motion sound `sound` of character table `table`
/// becomes `ids[material]` (`which` 0: the floor under the character),
/// heard by soldiers as noise level `noise[material]` (`FUN_0011b630`).
#[derive(Clone, Copy, Debug)]
pub struct SoundTable {
    pub table: u8,
    pub which: u8,
    pub sound: u16,
    pub ids: [u16; 8],
    pub noise: [u8; 8],
}

impl Banks {
    pub fn a32(&self, offset: u16) -> i32 {
        let o = offset as usize;
        i32::from_le_bytes(self.a[o..o + 4].try_into().unwrap())
    }

    pub fn set_a32(&mut self, offset: u16, v: i32) {
        let o = offset as usize;
        self.a[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn set_b32(&mut self, offset: u16, v: i32) {
        let o = offset as usize;
        self.b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn b16(&self, offset: u16) -> i16 {
        let o = offset as usize;
        self.b.get(o..o + 2).map_or(0, |b| i16::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn set_b16(&mut self, offset: u16, v: i16) {
        let o = offset as usize;
        self.b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
}

impl<'a> Vm<'a> {
    pub fn with_banks(gcx: &'a Gcx, banks: Banks) -> Self {
        Self {
            procs: gcx.procs.iter().map(|p| (p.id, p.body.as_slice())).collect(),
            bank_a: banks.a,
            bank_b: banks.b,
            spawns: Vec::new(),
            places: HashMap::new(),
            traps: Vec::new(),
            load: None,
            eager: false,
            timer: 0,
            song: 0,
            codec: Vec::new(),
            codec_mode: false,
            lines: Vec::new(),
            line_voices: Vec::new(),
            voice: 0,
            photos: Vec::new(),
            photo_done: None,
            strings: &gcx.strings,
            last: 0,
            depth: 0,
            rng: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() | 1),
        }
    }

    /// `rand n`: 0..n.
    fn rand(&mut self, n: i32) -> i32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        if n > 0 { (self.rng % n as u32) as i32 } else { 0 }
    }

    pub fn banks(&self) -> Banks {
        Banks { a: self.bank_a.clone(), b: self.bank_b.clone() }
    }

    /// The stage `load`ed, the actors created, and the variables.
    pub fn finish(self) -> (Option<String>, Vec<Spawn>, Vec<CodecReq>, Banks) {
        (self.load, self.spawns, self.codec, Banks { a: self.bank_a, b: self.bank_b })
    }

    /// Runs a proc with integer arguments (an actor's callback).
    pub fn call_proc(&mut self, proc: u32, args: &[i32]) {
        self.call(proc, args);
    }

    /// Runs a trap's block with `[box, actor, event, x, y, z, 0]`.
    pub fn run_block(&mut self, body: &[Stmt], args: &[i32]) {
        self.block(body, args);
    }

    fn hash(&mut self, v: Option<&Value>, args: &[i32]) -> u32 {
        match v.map(|v| self.value(v, args)) {
            Some(Val::Str(s)) => str_code(s.as_bytes()),
            Some(v) => v.int().unwrap_or(0) as u32 & 0x00FF_FFFF,
            None => 0,
        }
    }

    fn halted(&self) -> bool {
        self.eager && self.load.is_some()
    }

    pub fn run(&mut self, stmts: &[Stmt]) {
        self.block(stmts, &[]);
    }

    /// Returns `Some` when a `return` ends the block.
    fn block(&mut self, stmts: &[Stmt], args: &[i32]) -> Option<i32> {
        if self.depth > 64 {
            return None;
        }
        self.depth += 1;
        let mut result = None;
        for s in stmts {
            if self.halted() {
                break;
            }
            if let Some(r) = self.stmt(s, args) {
                result = Some(r);
                break;
            }
        }
        self.depth -= 1;
        result
    }

    fn stmt(&mut self, s: &Stmt, args: &[i32]) -> Option<i32> {
        match s {
            Stmt::Call { proc, args: a } => {
                let vals: Vec<i32> = a.iter().map(|v| self.value(v, args).int().unwrap_or(0)).collect();
                self.call(*proc, &vals);
                None
            }
            Stmt::Command { name, args: a, options } => self.command(*name, a, options, args),
        }
    }

    fn call(&mut self, proc: u32, args: &[i32]) {
        // GCL_TRACE=1: log every proc call (debugging script flow).
        static TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *TRACE.get_or_init(|| std::env::var_os("GCL_TRACE").is_some()) {
            eprintln!("gcl: call #{proc:06x} {args:?}");
        }
        if let Some(body) = self.procs.get(&proc).copied() {
            self.last = self.block(body, args).unwrap_or(0);
        }
    }

    fn command(&mut self, name: u32, a: &[Value], options: &[(u8, Vec<Value>)], args: &[i32]) -> Option<i32> {
        let is = |n: &str| name == str_code(n.as_bytes());
        if is("if") {
            // if <cond> {..} [-i <cond> {..}]* [-e {..}]
            let mut branches = vec![(Some(&a[0]), a.get(1))];
            for (l, v) in options {
                match l {
                    b'i' => branches.push((v.first(), v.get(1))),
                    b'e' => branches.push((None, v.first())),
                    _ => {}
                }
            }
            for (cond, body) in branches {
                let take = cond.is_none_or(|c| self.value(c, args).int().unwrap_or(0) != 0);
                if take {
                    if let Some(Value::Block(b)) = body {
                        return self.block(b, args);
                    }
                    return None;
                }
            }
            None
        } else if is("eval") {
            for v in a {
                self.last = self.value(v, args).int().unwrap_or(0);
            }
            None
        } else if is("return") {
            Some(a.first().map_or(0, |v| self.value(v, args).int().unwrap_or(0)))
        } else if is("chara") {
            let mut vals = a.iter().map(|v| self.value(v, args));
            let kind = match vals.next() {
                Some(Val::Str(s)) => str_code(s.as_bytes()) as i32,
                Some(v) => v.int()?,
                None => return None,
            };
            let spawn_args: Vec<Val> = vals.collect();
            let kind = kind as u32 & 0x00FF_FFFF;
            // A `delay`'s blocks see its `-a` values as their `$1`.. (the
            // door's walk-out passes the distance that way).
            let own: Option<Vec<i32>> = (kind == DELAY_ACTOR).then(|| {
                options
                    .iter()
                    .filter(|(l, _)| *l == b'a')
                    .flat_map(|(_, v)| v.iter())
                    .map(|x| self.value(x, args).int().unwrap_or(0))
                    .collect()
            });
            let block_args: Vec<i32> = own.unwrap_or_else(|| args.to_vec());
            let spawn = Spawn {
                kind,
                args: spawn_args,
                options: options
                    .iter()
                    .map(|(l, v)| (*l, v.iter().map(|x| if matches!(x, Value::Block(_)) { self.value(x, &block_args) } else { self.value(x, args) }).collect()))
                    .collect(),
            };
            let on_create: Vec<u32> = CALL_ON_CREATE
                .iter()
                .filter(|(k, _)| *k == spawn.kind)
                .flat_map(|(_, l)| spawn.opt(*l).unwrap_or(&[]).iter())
                .filter_map(|v| match v {
                    Val::Proc(p) => Some(*p),
                    _ => None,
                })
                .collect();
            let eager = (self.eager && EAGER.contains(&spawn.kind))
                .then(|| spawn.opt(b'p').and_then(|v| v.first().cloned()))
                .flatten();
            // A timer created by this run answers `#450c64` from now on.
            if spawn.kind == crate::timer::TIMER_ACTOR
                && let Some(t) = spawn.opt_int(b't', 0)
            {
                self.timer = t;
            }
            if spawn.kind == crate::crowd::CROWD_ACTOR
                && let Some(c) = spawn.opt(b'c')
            {
                for g in c.chunks_exact(6) {
                    if let Some(name) = g[5].int() {
                        let v = |i: usize| g[i].int().unwrap_or(0);
                        self.places.insert(name as u32 & 0x00FF_FFFF, [v(0), v(1), v(2)]);
                    }
                }
            }
            self.spawns.push(spawn);
            for p in on_create {
                self.call(p, &[]);
            }
            if let Some(Val::Proc(p)) = eager {
                self.call(p, &[]);
            }
            None
        } else if is("trap") {
            let opt = |l: u8| options.iter().find(|(o, _)| *o == l).map(|(_, v)| v);
            let area = self.hash(a.first(), args);
            let actor = self.hash(a.get(1), args);
            let mode = self.hash(opt(b'm').and_then(|v| v.first()), args);
            let dir = opt(b'd').cloned().map(|v| {
                let d = v.first().map_or(0, |x| self.value(x, args).int().unwrap_or(0));
                let r = v.get(1).map_or(0x400, |x| self.value(x, args).int().unwrap_or(0x400));
                (d, r)
            });
            let body = match opt(b'e').and_then(|v| v.first()) {
                Some(Value::Block(b)) => b.clone(),
                _ => Vec::new(),
            };
            self.traps.push(Trap { area, actor, mode, button: opt(b'b').is_some(), dir, body });
            None
        } else if is("load") {
            if self.load.is_none() {
                // `-g`: the game over scene (e.g. caught in the holds).
                self.load = if options.iter().any(|(l, _)| *l == b'g') {
                    Some(GAME_OVER.to_string())
                } else {
                    match a.first().map(|v| self.value(v, args)) {
                        Some(Val::Str(s)) => Some(s),
                        Some(Val::Name(h)) => Some(format!("#{h:06x}")),
                        _ => None,
                    }
                };
            }
            None
        } else if is("restart") {
            if self.load.is_none() {
                self.load = Some(RESTART.to_string());
            }
            None
        } else if is("rand") {
            let n = a.first().map_or(0, |v| self.value(v, args).int().unwrap_or(0));
            self.last = self.rand(n);
            None
        } else if is("command") && Self::is_foreach(a.first()) {
            // `command "foreach" -a n -r rows -d values.. -e { .. }`: the
            // block once per row of n values, which are its `$1`..`$n`.
            let opt = |l: u8| options.iter().find(|(o, _)| *o == l).map(|(_, v)| v);
            let n = opt(b'a').and_then(|v| v.first()).map_or(1, |v| self.value(v, args).int().unwrap_or(1)).max(1) as usize;
            let values: Vec<i32> = opt(b'd').map_or(Vec::new(), |v| v.iter().map(|x| self.value(x, args).int().unwrap_or(0)).collect());
            let rows = opt(b'r').and_then(|v| v.first()).map_or(values.len() / n, |v| self.value(v, args).int().unwrap_or(0) as usize);
            if let Some(Value::Block(b)) = opt(b'e').and_then(|v| v.first()) {
                for row in values.chunks(n).take(rows) {
                    if let Some(r) = self.block(b, row) {
                        return Some(r);
                    }
                }
            }
            self.last = 0;
            None
        } else if is("command") && matches!(a.first(), Some(Value::Name(CODEC_COMMAND))) {
            // `command #4a243a -o <event> <freq> <id>` (the call Snake gets on
            // this frequency here), `-c <freq> <id> <mode>` (an incoming
            // call), `-b <freq> <id>` (the stage's default call), `-r` (back
            // to the default).
            let ints = |s: &mut Self, v: &[Value]| v.iter().map(|x| s.value(x, args).int().unwrap_or(0)).collect::<Vec<i32>>();
            for (l, v) in options {
                let n = ints(self, v);
                let req = match (l, n.as_slice()) {
                    (b'o', [event, freq, id, ..]) => Some(CodecReq::Area { event: *event as u32, freq: *freq, id: *id as u32 }),
                    (b'c', [freq, id, ..]) => Some(CodecReq::Call { freq: *freq, id: *id as u32 }),
                    (b'b', [freq, id, ..]) => Some(CodecReq::Base { freq: *freq, id: *id as u32 }),
                    (b'r', _) => Some(CodecReq::Reset),
                    _ => None,
                };
                self.codec.extend(req);
            }
            self.last = 0;
            None
        } else if is("command") && matches!(a.first(), Some(Value::Name(h)) if *h & 0x00FF_FFFF == SOUND_AT_COMMAND) {
            // `command #ee8b90 -s <effect> -p x y z [-m mode]`: an effect at
            // a place (scripted events: impacts, explosions).
            let get = |s: &mut Self, l: u8, i: usize| {
                options.iter().find(|(o, _)| *o == l).and_then(|(_, v)| v.get(i)).and_then(|v| s.value(v, args).int())
            };
            if let Some(id) = get(self, b's', 0) {
                let at = [0, 1, 2].map(|i| get(self, b'p', i).unwrap_or(0));
                self.codec.push(CodecReq::SoundAt { id: id as u32, at });
            }
            self.last = 0;
            None
        } else if is("command") && matches!(a.first(), Some(Value::Name(h)) if *h & 0x00FF_FFFF == OVER_COMMAND) {
            let proc = |s: &mut Self, l: u8| {
                options.iter().find(|(o, _)| *o == l).and_then(|(_, v)| v.first()).and_then(|v| match s.value(v, args) {
                    Val::Proc(p) => Some(p),
                    _ => None,
                })
            };
            let (over, cont) = (proc(self, b'g'), proc(self, b'c'));
            self.codec.push(CodecReq::OverProcs { over, cont });
            self.last = 0;
            None
        } else if is("command") && matches!(a.first(), Some(Value::Name(h)) if *h & 0x00FF_FFFF == DEATH_VOICE_COMMAND) {
            if let Some(id) = options.iter().find(|(o, _)| *o == b'v').and_then(|(_, v)| v.first()).and_then(|v| self.value(v, args).int()) {
                self.codec.push(CodecReq::DeathVoice(id as u32));
            }
            self.last = 0;
            None
        } else if is("command") && matches!(a.first(), Some(Value::Name(h)) if *h & 0x00FF_FFFF == FLAGS_COMMAND) {
            // `command #944e11 -m "off" -r "on" ...` (`cmd_944e11`): turn
            // parts of the game off and on again around scripted scenes;
            // "off" sets the bit in DAT_0017f498, "on" clears it.
            let (mut on, mut off) = (0u32, 0u32);
            for (l, v) in options {
                let bit = match l {
                    b'm' => flags::MENUS,
                    b'r' => flags::RADAR,
                    b'g' => flags::GUN,
                    b's' => flags::S,
                    b'A' => flags::ACTION,
                    b'p' => flags::PAUSE,
                    _ => 0,
                };
                match v.first() {
                    Some(Value::Str(s)) if s == "on" => on |= bit,
                    Some(Value::Str(s)) if s == "off" => off |= bit,
                    _ => {}
                }
            }
            self.codec.push(CodecReq::Flags { on, off });
            self.last = 0;
            None
        } else if is("command") && let Some(id) = Self::command_id(a.first()) {
            self.special(id, a, options, args)
        } else if is("command") {
            // `command <id> <map> -s { ... }`: the setup block runs now.
            if let Some((_, v)) = options.iter().find(|(l, _)| *l == b's')
                && let Some(Value::Block(b)) = v.first() {
                    return self.block(b, args);
                }
            // Other commands' results are not modelled; read as 0.
            // MGS2_CMD_LOG=1: name them (finding what is still missing).
            if std::env::var_os("MGS2_CMD_LOG").is_some() {
                let opts: String = options.iter().map(|(l, _)| *l as char).collect();
                eprintln!("gcl: unmodelled command {:?} -{opts}", a.first());
            }
            self.last = 0;
            None
        } else if name & 0x00FF_FFFF == SEND_MESSAGE {
            // `#3822c7 <actor> <code> args..` (`FUN_0010a8a8` -> the message
            // send `FUN_00142a08`): the same message as `mesg`, e.g. a script
            // wall switched on or off.
            let target = a.first().map(|v| self.value(v, args));
            let code = a.get(1).map(|v| self.value(v, args));
            if let (Some(Val::Name(t)), Some(c)) = (target, code) {
                let code = match c {
                    Val::Name(c) => c,
                    v => v.int().unwrap_or(0) as u32,
                };
                let rest = a.iter().skip(2).map(|v| self.value(v, args).int().unwrap_or(0)).collect();
                self.codec.push(CodecReq::Mesg { target: t & 0x00FF_FFFF, code: code & 0x00FF_FFFF, args: rest });
            }
            None
        } else if is("mesg") && matches!(a.first(), Some(Value::Name(h)) if *h & 0x00FF_FFFF == crate::timer::TIMER_ACTOR) {
            if let Some(Value::Name(m)) = a.get(1) {
                self.codec.push(CodecReq::Timer(*m & 0x00FF_FFFF));
            }
            None
        } else if is("mesg") && a.len() >= 2 && !matches!(a.first(), Some(Value::Name(h)) if *h & 0x00FF_FFFF == SOUND_RELAY) {
            // Any other actor message (`mesg <actor> <code> args..`), for the
            // actors the port keeps (script walls, props, soldiers). The
            // actor may be given as a number or a variable (`mesg $2 ..`,
            // `mesg 90520060 ..`).
            let name = |v: Val| match v {
                Val::Name(n) => Some(n),
                v => v.int().map(|i| i as u32),
            };
            if let (Some(t), Some(c)) = (name(self.value(&a[0], args)), name(self.value(&a[1], args))) {
                let rest = a.iter().skip(2).map(|v| self.value(v, args).int().unwrap_or(0)).collect();
                self.codec.push(CodecReq::Mesg { target: t & 0x00FF_FFFF, code: c & 0x00FF_FFFF, args: rest });
            }
            None
        } else if is("mesg") && matches!(a.first(), Some(Value::Name(h)) if *h & 0x00FF_FFFF == SOUND_RELAY) {
            // `mesg #9b5b7f <target> <command>`: the sound relay actor
            // (`chara #c88afa #9b5b7f`) passes the command to the driver.
            if let Some(c) = a.get(2).and_then(|v| self.value(v, args).int()) {
                self.sound(c);
            }
            None
        } else if self.codec_mode && name == CODEC_LINE {
            // `#3b91eb <speaker> <?> -m str[n]`: one line of dialogue.
            let speaker = match a.first().map(|v| self.value(v, args)) {
                Some(v) => v.int().unwrap_or(0) as u32 & 0x00FF_FFFF,
                None => 0,
            };
            let text = options
                .iter()
                .find(|(l, _)| *l == b'm')
                .and_then(|(_, v)| match v.first() {
                    Some(Value::StrRef(i)) => self.strings.get(*i as usize).cloned(),
                    Some(Value::Str(s)) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            self.lines.push((speaker, text));
            self.line_voices.push(self.voice);
            None
        } else if self.codec_mode && name == CODEC_VOICE {
            self.voice = a.first().map_or(0, |v| self.value(v, args).int().unwrap_or(0)) as u32;
            for v in a.iter().skip(1).chain(options.iter().flat_map(|(_, v)| v)) {
                if let Value::Block(b) = v
                    && let Some(r) = self.block(b, args)
                {
                    self.voice = 0;
                    return Some(r);
                }
            }
            self.voice = 0;
            None
        } else if self.codec_mode && name == CODEC_LOOP {
            if let Some(Value::Block(b)) = a.first() {
                for _ in 0..16 {
                    if self.block(b, args).unwrap_or(0) == 0 {
                        break;
                    }
                }
            }
            None
        } else if self.codec_mode {
            // Other Codec commands (the call itself, voice clips, ...): run
            // their blocks in order.
            for v in a.iter().chain(options.iter().flat_map(|(_, v)| v)) {
                if let Value::Block(b) = v
                    && let Some(r) = self.block(b, args)
                {
                    return Some(r);
                }
            }
            None
        } else {
            // mesg, trap, print, load, restart, command, ...: not yet.
            if std::env::var_os("MGS2_CMD_LOG").is_some() {
                eprintln!("gcl: unmodelled #{name:06x}");
            }
            None
        }
    }

    /// The id of a `command` this VM models beyond the Codec.
    /// A sound driver command, noting song requests.
    fn sound(&mut self, c: i32) {
        let u = c as u32;
        if u.wrapping_sub(0x0100_0001) < 8 {
            self.song = c;
        }
        if u.wrapping_sub(0x0200_000A) < 4 {
            self.song = 0;
        }
        self.codec.push(CodecReq::Sound(u));
    }

    fn is_foreach(v: Option<&Value>) -> bool {
        let want = str_code(b"foreach") & 0x00FF_FFFF;
        match v {
            Some(Value::Name(h)) => *h & 0x00FF_FFFF == want,
            Some(Value::Str(s)) => s == "foreach",
            _ => false,
        }
    }

    fn command_id(v: Option<&Value>) -> Option<u32> {
        let id = match v? {
            Value::Name(h) => *h & 0x00FF_FFFF,
            Value::Str(s) => str_code(s.as_bytes()),
            _ => return None,
        };
        let known = CALL_AFTER.contains(&id)
            || CODEC_THEN.contains(&id)
            || [FILL_ARRAY, PHOTO_TARGET, PHOTO_DONE, SPEECH, VOLUME, SOUND_TABLE, ACTOR_POS, IMPACT_SOUND, IMPACT_SCARS, WEAPON_LIGHT, REPEAT, SOUND, WHEN_FREE, TIMER_LEFT, str_code(b"rand"), SIN, COS, ABS, DISTANCE, ATAN, SONG].contains(&id);
        known.then_some(id)
    }

    fn special(&mut self, id: u32, a: &[Value], options: &[(u8, Vec<Value>)], args: &[i32]) -> Option<i32> {
        let opt = |s: &mut Self, l: u8| -> Vec<Val> {
            options.iter().find(|(o, _)| *o == l).map(|(_, v)| v.iter().map(|x| s.value(x, args)).collect()).unwrap_or_default()
        };
        let proc = |vals: &[Val]| match vals.first() {
            Some(Val::Proc(p)) => Some(*p),
            _ => None,
        };
        self.last = 0;
        if id == SONG {
            self.last = self.song;
        } else if id == SIN || id == COS || id == ABS {
            // `cmd_01d98e` / `cmd_019a53`: sin / cos of a 4096-unit angle,
            // times 4096 (a Taylor series on VU0); `cmd_0190b3`: abs.
            let x = a.get(1).map_or(0, |v| self.value(v, args).int().unwrap_or(0));
            self.last = if id == ABS {
                x.wrapping_abs()
            } else {
                let turn = f64::from(x) / 4096.0 * std::f64::consts::TAU;
                let r = if id == SIN { turn.sin() } else { turn.cos() };
                (r * 4096.0) as i32
            };
        } else if id == DISTANCE || id == ATAN {
            let n: Vec<f32> = a[1..].iter().map(|v| self.value(v, args).int().unwrap_or(0) as f32).collect();
            let at = |i: usize| n.get(i).copied().unwrap_or(0.0);
            self.last = if id == DISTANCE {
                ((at(0) - at(3)).powi(2) + (at(1) - at(4)).powi(2) + (at(2) - at(5)).powi(2)).sqrt() as i32
            } else {
                ((at(1).atan2(at(0)) * 2048.0 / std::f32::consts::PI) as i32) & 0xFFF
            };
        } else if id == TIMER_LEFT {
            self.last = self.timer;
            if std::env::var_os("GCL_TRACE").is_some() {
                eprintln!("gcl: timer left {}", self.timer);
            }
        } else if id == str_code(b"rand") {
            let n = a.get(1).map_or(0, |v| self.value(v, args).int().unwrap_or(0));
            self.last = self.rand(n);
        } else if id == FILL_ARRAY {
            if let Some(Value::Var(var)) = a.get(1) {
                let (b, o, _) = self.slot(var, args);
                let size = match var.ty {
                    1 => 2,
                    6 | 8 | 9 => 4,
                    _ => 1,
                };
                for (i, v) in a[2..].iter().enumerate() {
                    let x = self.value(v, args).int().unwrap_or(0);
                    let bank = if b { &mut self.bank_b } else { &mut self.bank_a };
                    let at = o + i * size;
                    if let Some(dst) = bank.get_mut(at..at + size) {
                        dst.copy_from_slice(&x.to_le_bytes()[..size]);
                    }
                }
            }
        } else if id == PHOTO_TARGET {
            let p: Vec<f32> = opt(self, b'p').iter().map(|v| v.int().unwrap_or(0) as f32).collect();
            if p.len() >= 12
                && let Some(pr) = proc(&opt(self, b'R'))
            {
                let c = |i: usize| [p[i * 3], p[i * 3 + 1], p[i * 3 + 2]];
                let int = |s: &mut Self, l: u8, d: i32| opt(s, l).first().and_then(Val::int).unwrap_or(d);
                let (fill, pitch, yaw) = (int(self, b'f', 0) as f32, int(self, b'x', 640), int(self, b'y', 320));
                self.photos.push(PhotoTarget { corners: [c(0), c(1), c(2), c(3)], fill, proc: pr, pitch, yaw });
            }
        } else if id == WHEN_FREE {
            if self.eager
                && let Some((_, v)) = options.iter().find(|(l, _)| *l == b'e')
                && let Some(Value::Block(b)) = v.first()
            {
                return self.block(b, args);
            }
        } else if id == PHOTO_DONE {
            self.photo_done = proc(&opt(self, b'p'));
        } else if CODEC_THEN.contains(&id) {
            if let Some(cid) = opt(self, b'c').first().and_then(Val::int) {
                let then = proc(&opt(self, b'p'));
                self.codec.push(CodecReq::Then { id: cid as u32, then });
            }
        } else if id == SOUND {
            if let Some(c) = opt(self, b'c').first().and_then(Val::int) {
                self.sound(c);
            }
        } else if id == SPEECH {
            if let Some(sid) = a.get(1).and_then(|v| self.value(v, args).int()) {
                let then = proc(&opt(self, b'p'));
                self.codec.push(CodecReq::Speech { id: sid as u32, then });
                // The handle the game returns; the line's id stands in.
                self.last = sid;
            }
        } else if id == SOUND_TABLE {
            if options.iter().any(|(l, _)| *l == b'c') {
                self.codec.push(CodecReq::SoundTable(None));
            }
            let int = |s: &mut Self, l: u8| opt(s, l).first().and_then(Val::int);
            if let (Some(table), Some(sound)) = (int(self, b'i'), int(self, b's')) {
                let which = int(self, b't').unwrap_or(0);
                let d: Vec<i32> = opt(self, b'd').iter().filter_map(Val::int).collect();
                let mut ids = [0xFFFFu16; 8];
                let mut noise = [0u8; 8];
                for i in 0..8 {
                    if let Some(v) = d.get(i * 2) {
                        ids[i] = *v as u16;
                    }
                    if let Some(v) = d.get(i * 2 + 1) {
                        noise[i] = *v as u8;
                    }
                }
                self.codec.push(CodecReq::SoundTable(Some(SoundTable { table: table as u8, which: which as u8, sound: sound as u16, ids, noise })));
            }
        } else if id == ACTOR_POS {
            let name = match a.get(1).map(|v| self.value(v, args)) {
                Some(Val::Name(h)) => Some(h & 0x00FF_FFFF),
                Some(v) => v.int().map(|i| i as u32 & 0x00FF_FFFF),
                None => None,
            };
            self.last = -1;
            if let Some(at) = name.and_then(|n| self.places.get(&n).copied()) {
                for (k, c) in at.iter().enumerate() {
                    if let Some(Value::Var(var)) = a.get(2 + k) {
                        self.write(var, args, *c);
                    }
                }
                self.last = 1;
            }
        } else if id == REPEAT {
            let n = opt(self, b'a').first().and_then(Val::int).unwrap_or(0).max(0) as usize;
            let times = opt(self, b'r').first().and_then(Val::int).unwrap_or(0).max(0) as usize;
            let values: Vec<i32> = opt(self, b'd').iter().map(|v| v.int().unwrap_or(0)).collect();
            if let Some((_, v)) = options.iter().find(|(l, _)| *l == b'e')
                && let Some(Value::Block(b)) = v.first()
            {
                for i in 0..times.min(256) {
                    let call: Vec<i32> = (0..n).map(|k| values.get(i * n + k).copied().unwrap_or(0)).collect();
                    // Each pass runs like a call (a `return` ends only it).
                    self.block(b, &call);
                }
            }
        } else if id == IMPACT_SOUND || id == IMPACT_SCARS {
            if let Some(n) = opt(self, b'n').first().and_then(Val::int).filter(|n| (0..16).contains(n)) {
                self.codec.push(if id == IMPACT_SOUND { CodecReq::ImpactSound(n as u8) } else { CodecReq::ImpactScars(n as u8) });
            }
        } else if id == WEAPON_LIGHT {
            self.codec.push(CodecReq::WeaponLight);
        } else if id == VOLUME {
            if let Some(h) = a.get(1).and_then(|v| self.value(v, args).int()) {
                let vol = opt(self, b'v').first().and_then(Val::int).unwrap_or(0x3F);
                self.codec.push(CodecReq::Volume { handle: h as u32, vol: vol.clamp(0, 0x3F) as u8 });
            }
        } else if CALL_AFTER.contains(&id) && self.eager
            && let Some(p) = proc(&opt(self, b'p'))
        {
            self.call(p, &[]);
        }
        None
    }

    fn value(&mut self, v: &Value, args: &[i32]) -> Val {
        match v {
            Value::Int(_, i) => Val::Int(*i),
            Value::Name(h) => Val::Name(*h),
            Value::ProcRef(h) => Val::Proc(*h),
            Value::Str(s) => Val::Str(s.clone()),
            Value::StrRef(i) => Val::Int(i32::from(*i)),
            Value::Var(var) => Val::Int(self.read(var, args)),
            Value::Arg(0) => Val::Int(self.last),
            Value::Arg(n) => Val::Int(args.get(*n as usize - 1).copied().unwrap_or(0)),
            Value::Expr(items) => Val::Int(self.expr(items, args)),
            Value::Block(b) => Val::Int(self.block(b, args).unwrap_or(self.last)),
        }
    }

    /// (bank B?, byte offset, bit) — bit variables address `index + bit`
    /// bits from the base byte (`FUN_00153628`).
    fn slot(&mut self, var: &Var, args: &[i32]) -> (bool, usize, u8) {
        let index = var.index.as_ref().map_or(0, |i| self.value(i, args).int().unwrap_or(0)).max(0) as usize;
        if var.ty == 4 {
            let total = index + var.bit as usize;
            return (var.bank_b, var.offset as usize + total / 8, (total % 8) as u8);
        }
        let size = match var.ty {
            1 => 2,
            6 | 8 | 9 => 4,
            _ => 1,
        };
        (var.bank_b, var.offset as usize + index * size, 0)
    }

    fn read(&mut self, var: &Var, args: &[i32]) -> i32 {
        let (b, o, bit) = self.slot(var, args);
        let bank = if b { &self.bank_b } else { &self.bank_a };
        let get = |n: usize| bank.get(o..o + n).map(|s| s.to_vec()).unwrap_or(vec![0; n]);
        match var.ty {
            1 => i16::from_le_bytes(get(2).try_into().unwrap()).into(),
            4 => i32::from(bank.get(o).copied().unwrap_or(0) >> bit & 1),
            6 | 9 => i32::from_le_bytes(get(4).try_into().unwrap()),
            8 => i32::from_le_bytes(get(4).try_into().unwrap()) & 0x00FF_FFFF,
            _ => i32::from(get(1)[0]),
        }
    }

    fn write(&mut self, var: &Var, args: &[i32], v: i32) {
        let (b, o, bit) = self.slot(var, args);
        let bank = if b { &mut self.bank_b } else { &mut self.bank_a };
        let mut put = |bytes: &[u8]| {
            if let Some(dst) = bank.get_mut(o..o + bytes.len()) {
                dst.copy_from_slice(bytes);
            }
        };
        match var.ty {
            1 => put(&(v as i16).to_le_bytes()),
            4 => {
                if let Some(byte) = bank.get_mut(o) {
                    let m = 1u8 << bit;
                    *byte = if v != 0 { *byte | m } else { *byte & !m };
                }
            }
            6 | 9 => put(&v.to_le_bytes()),
            8 => put(&(v & 0x00FF_FFFF).to_le_bytes()[..3]),
            _ => put(&[v as u8]),
        }
    }

    fn expr(&mut self, items: &[ExprItem], args: &[i32]) -> i32 {
        // Stack of (value, variable it came from) so `=` can assign.
        let mut stack: Vec<(i32, Option<Var>)> = Vec::new();
        for it in items {
            match it {
                ExprItem::Push(Value::Var(var)) => {
                    let v = self.read(var, args);
                    stack.push((v, Some(var.clone())));
                }
                ExprItem::Push(v) => {
                    let x = self.value(v, args).int().unwrap_or(0);
                    stack.push((x, None));
                }
                ExprItem::Op(op) => {
                    let (b, _) = stack.pop().unwrap_or((0, None));
                    // Every operator pops two (`FUN_00154080`); the compiler
                    // puts a dummy 0 under a unary one's operand.
                    if op.is_unary() {
                        stack.pop();
                        stack.push((unary(*op, b), None));
                        continue;
                    }
                    let (a, avar) = stack.pop().unwrap_or((0, None));
                    if *op == Op::Assign {
                        if let Some(var) = avar {
                            self.write(&var, args, b);
                        }
                        stack.push((b, None));
                    } else {
                        stack.push((binary(*op, a, b), None));
                    }
                }
            }
        }
        stack.last().map_or(0, |s| s.0)
    }
}

fn unary(op: Op, b: i32) -> i32 {
    match op {
        Op::Neg => b.wrapping_neg(),
        Op::Not => i32::from(b == 0),
        Op::BitNot => !b,
        _ => b,
    }
}

/// `FUN_00153F40`.
fn binary(op: Op, a: i32, b: i32) -> i32 {
    match op {
        Op::Add => a.wrapping_add(b),
        Op::Sub => a.wrapping_sub(b),
        Op::Mul => a.wrapping_mul(b),
        Op::Div => a.checked_div(b).unwrap_or(0),
        Op::Mod => a.checked_rem(b).unwrap_or(0),
        Op::Shl => a.wrapping_shl(b as u32 & 31),
        Op::Shr => ((a as u32) >> (b as u32 & 31)) as i32,
        Op::Eq => i32::from(a == b),
        Op::Ne => i32::from(a != b),
        Op::Lt => i32::from(a < b),
        Op::Le => i32::from(a <= b),
        Op::Gt => i32::from(a > b),
        Op::Ge => i32::from(a >= b),
        Op::BitOr => a | b,
        Op::BitAnd => a & b,
        Op::BitXor => a ^ b,
        Op::Or => i32::from(a != 0 || b != 0),
        Op::And => i32::from(a != 0 && b != 0),
        Op::Second => b,
        _ => 0,
    }
}
