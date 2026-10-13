//! Player-vs-level collision against a stage's `.hzx` data.
//!
//! This is a straightforward reimplementation, not yet a port of the game's
//! own routines (`hzxd.c`): floors give the ground height under a point,
//! walls push a vertical cylinder out horizontally. All units are mm.

use bevy::math::{Vec2, Vec3};
use mgs2_formats::hzx::Hzx;

struct Floor {
    flags: u32,
    /// XZ outline.
    outline: Vec<Vec2>,
    point: Vec3,
    normal: Vec3,
    min: Vec2,
    max: Vec2,
}

struct Wall {
    a: Vec2,
    b: Vec2,
    bottom: f32,
    top: f32,
    flags: u32,
}

pub struct Collision {
    floors: Vec<Floor>,
    walls: Vec<Wall>,
    /// Walls the stage script adds (`chara #f91b9f`, `dsegment.c`), by
    /// actor name; they stop Snake only.
    script_walls: Vec<(u32, Wall)>,
}

/// A script wall's flag bit for "switched off" (`dsegment.c` messages 1 / 2
/// clear and set it).
const WALL_OFF: u32 = 8;

impl Collision {
    /// Uses the primary wall/floor set (the second set's purpose is unknown).
    pub fn new(hzx: &Hzx) -> Self {
        let mut floors = Vec::new();
        let mut walls = Vec::new();
        for g in hzx.areas.iter().flat_map(|a| &a.groups) {
            for (flags, poly) in g.floors.iter().flat_map(|f| f.polygons.iter().map(move |p| (f.flags, p))) {
                if poly.vertices.len() < 3 || poly.normal[1] < 0.3 {
                    continue; // ceilings and steep faces
                }
                let v: Vec<Vec3> = poly.vertices.iter().map(|v| Vec3::from(*v)).collect();
                let mut n = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
                if n.y < 0.0 {
                    n = -n;
                }
                if n.y < 0.3 {
                    continue;
                }
                let outline: Vec<Vec2> = v.iter().map(|p| Vec2::new(p.x, p.z)).collect();
                let (min, max) = outline.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
                floors.push(Floor { flags, outline, point: v[0], normal: n, min, max });
            }
            for w in &g.walls {
                for pair in w.points.windows(2) {
                    let ((a, ha), (b, hb)) = (pair[0], pair[1]);
                    walls.push(Wall {
                        a: Vec2::new(a[0], a[2]),
                        b: Vec2::new(b[0], b[2]),
                        bottom: a[1].min(b[1]),
                        top: (a[1] + ha).max(b[1] + hb),
                        flags: w.flags,
                    });
                }
            }
        }
        Self { floors, walls, script_walls: Vec::new() }
    }

    /// `chara #f91b9f <name> -p x1 y1 z1 h1 x2 y2 z2 h2 -a flags`: a wall
    /// from point 1 to point 2 (feet heights y, heights h above them).
    /// (One actor name can carry several segments, e.g. a corner.)
    pub fn add_script_wall(&mut self, name: u32, p: [i32; 8], flags: u32) {
        let w = script_wall(p, flags);
        let same = |o: &Wall| o.a == w.a && o.b == w.b && o.bottom == w.bottom && o.top == w.top;
        if !self.script_walls.iter().any(|(n, o)| *n == name && same(o)) {
            self.script_walls.push((name, w));
        }
    }

    /// A message to a script wall (`FUN_00291e30`): 0 removes it, 1 / 2
    /// switch it on / off, 3 moves it to new points, 5 shifts it.
    pub fn script_wall_message(&mut self, name: u32, code: u32, args: &[i32]) {
        if code == 0 {
            self.script_walls.retain(|(n, _)| *n != name);
            return;
        }
        for (_, w) in self.script_walls.iter_mut().filter(|(n, _)| *n == name) {
            match code {
                1 => w.flags &= !WALL_OFF,
                2 => w.flags |= WALL_OFF,
                3 if args.len() >= 8 => {
                    let flags = w.flags;
                    *w = script_wall(args[..8].try_into().unwrap(), flags);
                }
                5 if args.len() >= 3 => {
                    let d = Vec2::new(args[0] as f32, args[2] as f32);
                    (w.a, w.b) = (w.a + d, w.b + d);
                    (w.bottom, w.top) = (w.bottom + args[1] as f32, w.top + args[1] as f32);
                }
                _ => {}
            }
        }
    }

    /// The script's walls as (bottom a, bottom b, height, on) for drawing.
    pub fn script_wall_lines(&self) -> impl Iterator<Item = (Vec3, Vec3, f32, bool)> + '_ {
        self.script_walls.iter().map(|(_, w)| {
            (Vec3::new(w.a.x, w.bottom, w.a.y), Vec3::new(w.b.x, w.bottom, w.b.y), w.top - w.bottom, w.flags & WALL_OFF == 0)
        })
    }

    /// How many walls the script has placed.
    pub fn script_wall_count(&self) -> usize {
        self.script_walls.len()
    }

    /// Pushes Snake out of the script's walls that are switched on.
    pub fn push_out_script(&self, pos: Vec3, radius: f32, height: f32, step: f32) -> Vec3 {
        if std::env::var_os("MGS2_NO_SCRIPT_WALLS").is_some() {
            return pos;
        }
        let walls: Vec<&Wall> = self.script_walls.iter().map(|(_, w)| w).filter(|w| w.flags & WALL_OFF == 0).collect();
        push_out(walls.into_iter(), pos, radius, height, step)
    }

    /// Highest floor at `(x, z)` that is no higher than `max_y`.
    pub fn floor_below(&self, x: f32, z: f32, max_y: f32) -> Option<f32> {
        self.floor_at(x, z, max_y).map(|f| f.0)
    }

    /// The material of the floor under `(x, z)`: the top four bits of its
    /// element flags (the soldiers' `FUN_00257bc0` passes `flags >> 28` to the motion
    /// sounds, which pick a footstep variant by it).
    pub fn floor_material(&self, x: f32, z: f32, max_y: f32) -> Option<u32> {
        self.floor_at(x, z, max_y).map(|f| f.1 >> 28)
    }

    fn floor_at(&self, x: f32, z: f32, max_y: f32) -> Option<(f32, u32)> {
        let p = Vec2::new(x, z);
        self.floors
            .iter()
            .filter(|f| p.cmpge(f.min).all() && p.cmple(f.max).all() && inside(&f.outline, p))
            .map(|f| (f.point.y - (f.normal.x * (x - f.point.x) + f.normal.z * (z - f.point.z)) / f.normal.y, f.flags))
            .filter(|y| y.0 <= max_y)
            .max_by(|a, b| a.0.total_cmp(&b.0))
    }

    /// Pushes a cylinder (feet at `pos.y`) out of walls that overlap its
    /// height, ignoring the lowest `step` mm so kerbs can be climbed.
    pub fn push_out(&self, pos: Vec3, radius: f32, height: f32, step: f32) -> Vec3 {
        push_out(self.walls.iter(), pos, radius, height, step)
    }
}

fn script_wall(p: [i32; 8], flags: u32) -> Wall {
    let [x1, y1, z1, h1, x2, y2, z2, h2] = p.map(|v| v as f32);
    Wall { a: Vec2::new(x1, z1), b: Vec2::new(x2, z2), bottom: y1.min(y2), top: (y1 + h1).max(y2 + h2), flags }
}

fn push_out<'a>(walls: impl Iterator<Item = &'a Wall> + Clone, pos: Vec3, radius: f32, height: f32, step: f32) -> Vec3 {
    let mut p = Vec2::new(pos.x, pos.z);
    let (lo, hi) = (pos.y + step, pos.y + height);
    for _ in 0..4 {
        let mut moved = false;
        for w in walls.clone().filter(|w| w.top > lo && w.bottom < hi) {
            let ab = w.b - w.a;
            let len2 = ab.length_squared();
            if len2 < 1.0 {
                continue;
            }
            let t = ((p - w.a).dot(ab) / len2).clamp(0.0, 1.0);
            let closest = w.a + ab * t;
            let d = p - closest;
            let dist = d.length();
            if dist < radius {
                let dir = if dist > 1e-3 { d / dist } else { Vec2::new(-ab.y, ab.x).normalize() };
                p = closest + dir * radius;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    Vec3::new(p.x, pos.y, p.y)
}

impl Collision {
    /// Whether a wall blocks the straight line from `a` to `b` (eye to eye).
    /// Walls are vertical, so the test is 2D with a height check at the
    /// crossing point.
    pub fn line_blocked(&self, a: Vec3, b: Vec3) -> bool {
        self.ray(a, b).is_some()
    }

    /// First wall or floor crossed by the segment `a`..`b`, as a fraction of
    /// its length. Floors block too, so nobody sees through a deck.
    pub fn ray(&self, a: Vec3, b: Vec3) -> Option<f32> {
        self.ray_masked(a, b, 0, 0)
    }

    /// Line of sight as `FUN_0014f230` tests it: walls and floors whose
    /// flags share a bit with the masks are seen through.
    pub fn ray_masked(&self, a: Vec3, b: Vec3, wall_mask: u32, floor_mask: u32) -> Option<f32> {
        let (p, r) = (Vec2::new(a.x, a.z), Vec2::new(b.x - a.x, b.z - a.z));
        let walls = self.wall_hits(a, b).filter(|(_, f)| f & wall_mask == 0).map(|(t, _)| t).min_by(f32::total_cmp);
        let d = b - a;
        let (lo, hi) = (p.min(p + r), p.max(p + r));
        let floors = self.floors.iter().filter(|f| f.flags & floor_mask == 0).filter_map(|f| {
            if hi.cmplt(f.min).any() || lo.cmpgt(f.max).any() {
                return None;
            }
            let denom = f.normal.dot(d);
            if denom.abs() < 1e-6 {
                return None;
            }
            let t = f.normal.dot(f.point - a) / denom;
            if !(0.0..=1.0).contains(&t) {
                return None;
            }
            let h = a + d * t;
            inside(&f.outline, Vec2::new(h.x, h.z)).then_some(t)
        });
        walls.into_iter().chain(floors).min_by(f32::total_cmp)
    }


    /// Where the segment `a`..`b` first meets a wall or floor, with the
    /// surface's normal facing `a`.
    pub fn ray_normal(&self, a: Vec3, b: Vec3) -> Option<(f32, Vec3)> {
        let d = b - a;
        let r = Vec2::new(d.x, d.z);
        let walls = self.walls.iter().filter_map(|w| {
            let s = w.b - w.a;
            let denom = r.perp_dot(s);
            if denom.abs() < 1e-6 {
                return None;
            }
            let q = w.a - Vec2::new(a.x, a.z);
            let (t, u) = (q.perp_dot(s) / denom, q.perp_dot(r) / denom);
            if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&u) {
                return None;
            }
            let y = a.y + d.y * t;
            if !(y > w.bottom && y < w.top) {
                return None;
            }
            let n = Vec3::new(-s.y, 0.0, s.x).normalize_or_zero();
            Some((t, if n.dot(d) > 0.0 { -n } else { n }))
        });
        let floors = self.floors.iter().filter_map(|f| {
            let denom = f.normal.dot(d);
            if denom.abs() < 1e-6 {
                return None;
            }
            let t = f.normal.dot(f.point - a) / denom;
            if !(0.0..=1.0).contains(&t) {
                return None;
            }
            let h = a + d * t;
            let hp = Vec2::new(h.x, h.z);
            (hp.cmpge(f.min).all() && hp.cmple(f.max).all() && inside(&f.outline, hp)).then(|| (t, if denom > 0.0 { -f.normal } else { f.normal }))
        });
        walls.chain(floors).min_by(|x, y| x.0.total_cmp(&y.0))
    }

    /// Walls crossed by the segment `a`..`b`: (fraction, wall flags).
    pub fn wall_hits(&self, a: Vec3, b: Vec3) -> impl Iterator<Item = (f32, u32)> + '_ {
        let (p, r) = (Vec2::new(a.x, a.z), Vec2::new(b.x - a.x, b.z - a.z));
        self.walls.iter().filter_map(move |w| {
            let s = w.b - w.a;
            let denom = r.perp_dot(s);
            if denom.abs() < 1e-6 {
                return None;
            }
            let q = w.a - p;
            let t = q.perp_dot(s) / denom;
            let u = q.perp_dot(r) / denom;
            if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&u) {
                return None;
            }
            let y = a.y + (b.y - a.y) * t;
            (y > w.bottom && y < w.top).then_some((t, w.flags))
        })
    }
}

/// A ledge Snake can hang from: the `.hzx` second wall set's segments with
/// flag 0x800, which run 200-250 mm below a deck edge under its railing
/// (their top is the deck floor). Units mm.
#[derive(Clone, Debug)]
pub struct HangLine {
    pub a: Vec3,
    pub b: Vec3,
    /// Horizontal unit normal pointing away from the deck (where Snake
    /// hangs).
    pub out: Vec2,
}

pub const HANG_FLAG: u32 = 0x800;

impl HangLine {
    /// Parameter (0..1) and horizontal distance of the closest point to `p`.
    pub fn closest(&self, p: Vec3) -> (f32, f32) {
        let (a, b, q) = (Vec2::new(self.a.x, self.a.z), Vec2::new(self.b.x, self.b.z), Vec2::new(p.x, p.z));
        let ab = b - a;
        let t = if ab.length_squared() > 1.0 { ((q - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        (t, (a + ab * t).distance(q))
    }

    pub fn length(&self) -> f32 {
        Vec2::new(self.b.x - self.a.x, self.b.z - self.a.z).length()
    }
}

pub fn hang_lines(hzx: &Hzx, col: &Collision) -> Vec<HangLine> {
    let mut out = Vec::new();
    for g in hzx.areas.iter().flat_map(|a| &a.groups) {
        for w in g.walls2.iter().filter(|w| w.flags & HANG_FLAG != 0) {
            for pair in w.points.windows(2) {
                let ((pa, ha), (pb, hb)) = (pair[0], pair[1]);
                let a = Vec3::new(pa[0], pa[1] + ha, pa[2]);
                let b = Vec3::new(pb[0], pb[1] + hb, pb[2]);
                let d = Vec2::new(b.x - a.x, b.z - a.z).normalize_or_zero();
                if d == Vec2::ZERO {
                    continue;
                }
                // The deck is on the side with floor at the ledge's top.
                let n = Vec2::new(-d.y, d.x);
                let mid = (a + b) * 0.5;
                let deck = |s: f32| {
                    let p = Vec2::new(mid.x, mid.z) + n * s * 600.0;
                    col.floor_below(p.x, p.y, mid.y + 100.0).is_some_and(|y| (y - mid.y).abs() < 150.0)
                };
                let out_n = if deck(1.0) && !deck(-1.0) { -n } else { n };
                let line = HangLine { a, b, out: out_n };
                if !out.iter().any(|l: &HangLine| l.a == line.a && l.b == line.b) {
                    out.push(line);
                }
            }
        }
    }
    out
}

/// Even-odd point-in-polygon test.
fn inside(poly: &[Vec2], p: Vec2) -> bool {
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
        j = i;
    }
    c
}
