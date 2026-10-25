//! The stage scripts' cameras (`chara 0x43f718 <name> -c <type> ... -s
//! <on/off>`), usually switched by traps as Snake moves between areas.
//!
//! - `-c 0 -b x y z x y z -l x y z x y z -r pitch yaw roll -f dist`: follow
//!   Snake from `dist` mm at the given angles (4096 = full turn), with the
//!   camera position clamped into box `-b` and the point looked at into box
//!   `-l`. A flat box makes a rail.
//! - `-c 1 -p x y z -t x y z`: a fixed camera at `-p` looking at `-t`
//!   (`FUN_0010c3f0` type 1 only recomputes its angles from the two).
//! - `-a n`: the lens, zoom n / 100 (+0x20; the projection's scale,
//!   `FUN_0011e928`: the 512-wide screen's half width over the focal
//!   length is 100 / n, its 448-line height's 224 / (256 x 1.121 x n / 100)).
//!   The view eases to it by a quarter a frame (`FUN_0010b1c8`).
//! - `-z 0x2000000 -N n` (`cmd_43f718`): the camera keeps its last place
//!   and moves to the new one over n frames, a 1/n-left step a frame (a
//!   straight line; `FUN_0010c4b8`), zoom too. `#de7ce7` pans this way.
//! - `-s`: 1 or an enter/stay trap event switches it on, -1 or the leave
//!   event off.
//!
//! Which one shows: `chara #061ade <name> -l n -p m` makes a camera in
//! group `DAT_0017f650`[n] (0, 3, 6, 8), listed by `-p` (`FUN_0010caa0`,
//! a new one before equal ones); each frame the first switched-on camera
//! of the lowest group shows (`FUN_0010af18`). So an event camera (`-l 0`)
//! stays up while the area traps keep switching theirs (`-l 2`) on.
//! Cameras the port never saw made rank last, the latest switched on first.
//!
//! A camera made inside a `delay` waits its frames ([`Spawn::after`]).
//! Not ported: the `-i` blends between cameras, `-E`, the `-b` / `-l`
//! boxes of type 1 and types 2-5.

use bevy::prelude::*;

use crate::gcl::{Spawn, trap_event};

/// Frames a second the game's camera steps at.
const FPS: f32 = 60.0;

pub const CAMERA_ACTOR: u32 = 0x43F718;
/// `chara #061ade <name> -c n -l group -p order` (`cmd_061ade`).
pub const CAMERA_MAKER: u32 = 0x061ADE;
/// `DAT_0017f650`: the group of each `-l`.
const GROUPS: [i32; 4] = [0, 3, 6, 8];

#[derive(Clone, Debug)]
pub enum ScriptCam {
    Follow { bmin: Vec3, bmax: Vec3, lmin: Vec3, lmax: Vec3, pitch: f32, yaw: f32, dist: f32 },
    Fixed { pos: Vec3, target: Vec3 },
}

/// The stage cameras, and the one showing.
#[derive(Default)]
pub struct Cameras {
    /// The camera showing, by actor name.
    pub active: Option<(u32, ScriptCam)>,
    /// The showing camera's `-a` zoom.
    pub zoom: Option<f32>,
    /// Each camera switched on: its place and lens (`-a` kept between
    /// switches), and when it was switched on.
    on: Vec<(u32, ScriptCam, Option<f32>, u64)>,
    /// Lenses of cameras switched off (a camera keeps its `-a`).
    lens: std::collections::HashMap<u32, f32>,
    /// `#061ade` ranks: name -> (group, order, when made).
    ranks: std::collections::HashMap<u32, (i32, i32, u64)>,
    clock: u64,
    /// A `-z 0x2000000` move under way, and whose.
    pan: Option<Pan>,
    pan_of: u32,
    /// Cameras made inside `delay`s: seconds left and the spawn.
    pending: Vec<(f32, Spawn)>,
}

/// Where a camera moving to its new place started, and how far it is.
#[derive(Clone, Debug)]
struct Pan {
    eye: Vec3,
    look: Vec3,
    zoom: Option<f32>,
    frames: f32,
    done: f32,
}

/// Bevy's vertical field of view for a camera zoom (`-a` / 100).
pub fn zoom_fov(zoom: f32) -> f32 {
    2.0 * (224.0 / (256.0 * 1.121 * zoom.max(0.1))).atan()
}

/// Counts down the stage cameras' waits and moves.
pub fn tick(time: Res<Time>, mut game: ResMut<crate::Game>) {
    if game.cameras.busy() {
        game.cameras.tick(time.delta_secs());
    }
}

fn angle(v: i32) -> f32 {
    v as f32 / 4096.0 * std::f32::consts::TAU
}

impl Cameras {
    /// Applies camera actors created by a script run, in order; those
    /// made inside `delay`s wait their frames.
    pub fn apply(&mut self, spawns: &[Spawn]) {
        for s in spawns.iter().filter(|s| s.kind == CAMERA_ACTOR || s.kind == CAMERA_MAKER) {
            if s.kind == CAMERA_MAKER {
                if let Some(name) = s.args.first().and_then(|v| v.int()) {
                    let group = s.opt_int(b'l', 0).and_then(|l| GROUPS.get(l as usize).copied()).unwrap_or(0);
                    self.clock += 1;
                    self.ranks.entry(name as u32).or_insert((group, s.opt_int(b'p', 0).unwrap_or(0), self.clock));
                }
                continue;
            }
            if s.after > 0 {
                let wait = s.after as f32 / FPS;
                // Trap "stay" events run their bodies every frame: one
                // copy of a waiting camera is enough.
                let dup = self.pending.iter().any(|(t, p)| (t - wait).abs() < 1.5 / FPS && p.args == s.args && p.options == s.options);
                if !dup {
                    self.pending.push((wait, s.clone()));
                }
            } else {
                self.apply_one(s);
            }
        }
    }

    fn busy(&self) -> bool {
        self.pan.is_some() || !self.pending.is_empty()
    }

    /// Counts down the waiting cameras and the move under way.
    pub fn tick(&mut self, dt: f32) {
        if let Some(p) = self.pan.as_mut() {
            p.done += dt * FPS;
            if p.done >= p.frames {
                self.pan = None;
            }
        }
        if self.pending.is_empty() {
            return;
        }
        for (t, _) in &mut self.pending {
            *t -= dt;
        }
        let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending).into_iter().partition(|(t, _)| *t <= 0.0);
        self.pending = wait;
        for (_, s) in due {
            self.apply_one(&s);
        }
    }

    /// Eye, look-at point (mm) and zoom for Snake's chest at `snake`.
    pub fn view(&self, snake: Vec3) -> Option<(Vec3, Vec3, Option<f32>)> {
        let (_, cam) = self.active.as_ref()?;
        let (eye, look) = cam.place(snake);
        let Some(p) = &self.pan else { return Some((eye, look, self.zoom)) };
        let t = (p.done / p.frames).clamp(0.0, 1.0);
        let zoom = match (p.zoom, self.zoom) {
            (Some(a), Some(b)) => Some(a + (b - a) * t),
            (_, z) => z,
        };
        Some((p.eye.lerp(eye, t), p.look.lerp(look, t), zoom))
    }

    fn apply_one(&mut self, s: &Spawn) {
        let Some(name) = s.args.first().and_then(|v| v.int()) else { return };
        let name = name as u32;
        let on = match s.opt_int(b's', 0) {
            Some(1) => true,
            Some(e) if e as u32 == trap_event::ENTER || e as u32 == trap_event::STAY => true,
            Some(_) => false,
            None => true,
        };
        if !on {
            if let Some(i) = self.on.iter().position(|o| o.0 == name) {
                let (_, _, zoom, _) = self.on.remove(i);
                if let Some(z) = zoom {
                    self.lens.insert(name, z);
                }
                if self.active.as_ref().is_some_and(|(n, _)| *n == name) {
                    self.pan = None;
                }
                self.choose();
            }
            return;
        }
        let v3 = |l: u8, i: usize| -> Option<Vec3> {
            Some(Vec3::new(s.opt_int(l, i)? as f32, s.opt_int(l, i + 1)? as f32, s.opt_int(l, i + 2)? as f32))
        };
        let cam = match s.opt_int(b'c', 0) {
            Some(0) => (|| {
                let (b0, b1, l0, l1) = (v3(b'b', 0)?, v3(b'b', 3)?, v3(b'l', 0)?, v3(b'l', 3)?);
                Some(ScriptCam::Follow {
                    bmin: b0.min(b1),
                    bmax: b0.max(b1),
                    lmin: l0.min(l1),
                    lmax: l0.max(l1),
                    pitch: angle(s.opt_int(b'r', 0)?),
                    yaw: angle(s.opt_int(b'r', 1)?),
                    dist: s.opt_int(b'f', 0)? as f32,
                })
            })(),
            Some(1) => (|| Some(ScriptCam::Fixed { pos: v3(b'p', 0)?, target: v3(b't', 0)? }))(),
            _ => None,
        };
        let Some(cam) = cam else { return };
        let old = self.on.iter().position(|o| o.0 == name).map(|i| self.on.remove(i));
        // Without `-a` the camera keeps its lens.
        let kept = old.as_ref().and_then(|o| o.2).or(self.lens.get(&name).copied());
        let zoom = s.opt_int(b'a', 0).map(|a| a as f32 / 100.0).or(kept);
        // `-z 0x2000000 -N n`: from where this camera was, over n frames.
        let frames = s.opt_int(b'N', 0).unwrap_or(0);
        let moving = s.opt_int(b'z', 0).is_some_and(|z| z & 0x200_0000 != 0) && frames > 0;
        let showing = self.active.as_ref().is_some_and(|(n, _)| *n == name);
        match &old {
            Some((_, ScriptCam::Fixed { pos, target }, z, _)) if moving => {
                eprintln!("stage camera {name:06x} pans over {frames} frames");
                self.pan = Some(Pan { eye: *pos, look: *target, zoom: *z, frames: frames as f32, done: 0.0 });
                self.pan_of = name;
            }
            // Set again (a trap's "stay" every frame): a move goes on.
            Some(_) if self.pan_of == name && !moving => {}
            _ if showing || self.pan_of == name => self.pan = None,
            _ => {}
        }
        // A camera switched on again keeps its place in the list.
        let when = old.map_or_else(
            || {
                self.clock += 1;
                self.clock
            },
            |o| o.3,
        );
        self.on.push((name, cam, zoom, when));
        self.choose();
    }

    /// Shows the first switched-on camera of the lowest group.
    fn choose(&mut self) {
        let key = |o: &(u32, ScriptCam, Option<f32>, u64)| match self.ranks.get(&o.0) {
            Some(&(g, p, made)) => (g, p, u64::MAX - made),
            None => (i32::MAX, 0, u64::MAX - o.3),
        };
        let best = self.on.iter().min_by_key(|o| key(o)).cloned();
        if best.as_ref().map(|b| b.0) != Some(self.pan_of) {
            self.pan = None;
        }
        self.zoom = best.as_ref().and_then(|b| b.2);
        self.active = best.map(|b| (b.0, b.1));
    }
}

impl ScriptCam {
    /// Eye and look-at point (mm) for Snake's chest at `snake`.
    pub fn place(&self, snake: Vec3) -> (Vec3, Vec3) {
        match self {
            ScriptCam::Follow { bmin, bmax, lmin, lmax, pitch, yaw, dist } => {
                // The camera faces along (yaw, pitch down); it sits `dist`
                // behind the look-at point.
                let facing = Vec3::new(yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
                let look = snake.clamp(*lmin, *lmax);
                let eye = (snake - facing * *dist).clamp(*bmin, *bmax);
                (eye, look)
            }
            ScriptCam::Fixed { pos, target } => (*pos, *target),
        }
    }
}
