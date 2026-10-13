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
/// as far as stage setup needs: `0xddf5ca` (enemy manager; `-w` creates the
/// "wccomm" actor), `0x59eda8` "wccomm" (`-e` creates the guards) and
/// `0x5bb4d4` (a reinforcement squad, `FUN_0029dcb8`: `-e` creates its
/// troopers).
const CALL_ON_CREATE: &[(u32, u8)] = &[(0xDDF5CA, b'w'), (0x59EDA8, b'e'), (0x5BB4D4, b'e')];
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
const EAGER: &[u32] = &[0xE22388, 0x743C9F];

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
    pub traps: Vec<Trap>,
    /// Stage named by the first `load` reached.
    pub load: Option<String>,
    /// Run timer/fade callbacks immediately and stop at `load`.
    pub eager: bool,
    /// Frames left on the holds' timer (`command #450c64`).
    pub timer: i32,
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
/// Snake's life (Bs16:0xf2; the maximum is Bs16:0xf4).
pub const SNAKE_LIFE: u16 = 0xF2;
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

#[derive(Clone, Copy, Debug)]
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
    Reset,
    /// Features switched on and off (`cmd_944e11`, [`flags`]).
    Flags { on: u32, off: u32 },
    /// The game-over and continue procs (`#80b977 -g -c`).
    OverProcs { over: Option<u32>, cont: Option<u32> },
    /// The voice for the game-over screen (`#5a4652 -v`).
    DeathVoice(u32),
    /// A raw sound driver command (`#f2eedc -c`).
    Sound(u32),
    /// A message to the holds' timer: 0 pause, 1 run.
    Timer(u32),
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
            traps: Vec::new(),
            load: None,
            eager: false,
            timer: 0,
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
            let spawn = Spawn {
                kind: kind as u32 & 0x00FF_FFFF,
                args: vals.collect(),
                options: options.iter().map(|(l, v)| (*l, v.iter().map(|x| self.value(x, args)).collect())).collect(),
            };
            let on_create = CALL_ON_CREATE
                .iter()
                .find(|(k, _)| *k == spawn.kind)
                .and_then(|(_, l)| spawn.opt(*l)?.first().cloned());
            let eager = (self.eager && EAGER.contains(&spawn.kind))
                .then(|| spawn.opt(b'p').and_then(|v| v.first().cloned()))
                .flatten();
            // A timer created by this run answers `#450c64` from now on.
            if spawn.kind == crate::timer::TIMER_ACTOR
                && let Some(t) = spawn.opt_int(b't', 0)
            {
                self.timer = t;
            }
            self.spawns.push(spawn);
            if let Some(Val::Proc(p)) = on_create {
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
            self.last = 0;
            None
        } else if is("mesg") && matches!(a.first(), Some(Value::Name(h)) if *h & 0x00FF_FFFF == crate::timer::TIMER_ACTOR) {
            if let Some(Value::Name(m)) = a.get(1) {
                self.codec.push(CodecReq::Timer(*m & 0x00FF_FFFF));
            }
            None
        } else if is("mesg") && matches!(a.first(), Some(Value::Name(h)) if *h & 0x00FF_FFFF == SOUND_RELAY) {
            // `mesg #9b5b7f <target> <command>`: the sound relay actor
            // (`chara #c88afa #9b5b7f`) passes the command to the driver.
            if let Some(c) = a.get(2).and_then(|v| self.value(v, args).int()) {
                self.codec.push(CodecReq::Sound(c as u32));
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
            None
        }
    }

    /// The id of a `command` this VM models beyond the Codec.
    fn command_id(v: Option<&Value>) -> Option<u32> {
        let id = match v? {
            Value::Name(h) => *h & 0x00FF_FFFF,
            Value::Str(s) => str_code(s.as_bytes()),
            _ => return None,
        };
        let known = CALL_AFTER.contains(&id)
            || CODEC_THEN.contains(&id)
            || [FILL_ARRAY, PHOTO_TARGET, PHOTO_DONE, SPEECH, SOUND, WHEN_FREE, TIMER_LEFT, str_code(b"rand")].contains(&id);
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
        if id == TIMER_LEFT {
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
                self.codec.push(CodecReq::Sound(c as u32));
            }
        } else if id == SPEECH {
            if let Some(sid) = a.get(1).and_then(|v| self.value(v, args).int()) {
                let then = proc(&opt(self, b'p'));
                self.codec.push(CodecReq::Speech { id: sid as u32, then });
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
