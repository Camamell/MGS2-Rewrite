//! `.hzx` stage collision (file type 7, letter `h`; `hzxd.c`).
//!
//! Loader: `FUN_00144510` -> `FUN_00144008` (pointer fix-up and conversion
//! of elements to runtime records by `FUN_00143f60` / `FUN_00143f90`).
//!
//! ```text
//! header
//!   +00 u16 ?            +02 u16 area count
//!   +04 u16 count A      +06 u16 count B
//!   +08 u32 -> A[count A] (12-byte records)
//!   +0C u32 -> ?          +10 u32 -> B[count B] (0x30-byte records)
//!   +14 u32 -> ?          +18 u32 -> ?
//!   +1C u32 -> areas (0x5C bytes each)
//! area
//!   +00..+30 flags/pointers to further data (not yet decoded; possibly AI
//!            navigation)
//!   +34 i32 grid origin xyz   +40 i32 cell size xyz
//!   +4C u8 grid dims x, y, z  +50 u16 group count   +54 u32 -> groups
//! group (0x34 bytes): one grid cell
//!   +00 i32 centre x, z, y (note the order; `FUN_00143F90` adds +08 to
//!       element y). Element coordinates are relative to this centre.
//!   +14 u16 counts: walls, floors, walls2, floors2, traps
//!   +1E u16 cell index
//!   +20 u32 -> element lists (same order; traps are [`TrapBox`] records)
//! element (0x20 bytes)
//!   +00 i16 bbox min xyz +06 i16 vertex count  +08 i16 bbox max xyz
//!   +0E i16 polygon size (floors: 3 or 4)
//!   +10 u32 -> vertices: i16 x, y, z, w
//!   +14 u32 flags
//! ```
//!
//! Walls are point lists where `w` is the wall height above the point.
//! Floors are lists of polygons (`polygon size` vertices each); on each
//! polygon's first three vertices `w` holds the plane normal * 32000.
//! The second element set is registered with ids offset by 0x400; what it is
//! used for (camera, bullets, ...) is not yet known.

use crate::{Result, format_err};

#[derive(Clone, Debug)]
pub struct Hzx {
    pub areas: Vec<Area>,
    /// Patrol routes (header table at +08), indexed by the guard's `-r`.
    pub routes: Vec<Route>,
    /// Named trigger boxes for the script's `trap` command, from every
    /// group's fifth element list (deduplicated across grid cells).
    pub traps: Vec<TrapBox>,
}

/// One 20-byte trap record: `i16 min x, y, z, pad; i16 max x, y, z, pad;
/// u32 name` (coordinates relative to the group centre; the pads differ
/// between copies of the same box in different cells). The trap
/// test checks `min <= p < max` on each axis and reports
/// the name to the `trap` handlers (`FUN_0014A448`; the box loop is the
/// function just before `FUN_00147EB8`).
#[derive(Clone, Debug, PartialEq)]
pub struct TrapBox {
    pub name: u32,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Route {
    pub points: Vec<Waypoint>,
}

/// One 40-byte route point, as the soldier's route loader `FUN_002593f0`
/// copies it: `f32 x, z, y, look_x, look_z, look_y; u16 action, wait, dir,
/// ?; u32 flags, area`.
#[derive(Clone, Copy, Debug)]
pub struct Waypoint {
    pub position: [f32; 3],
    /// The point the flags 0x5 / 0xa / 0x300 look at (runtime +0x400).
    pub look: [f32; 3],
    /// Action index (+0x18, runtime +0x900): the walker's action through
    /// the table at 0x1f39f0, or (flags 0x40 / 0x80) a motion number.
    pub action: i16,
    /// Frames to wait here (runtime +0x980): 0 walk on, -1 until the
    /// action ends, -2 for good, -3 until the script's message 2, -4 the
    /// manager's `-f` line for this point.
    pub wait: i16,
    /// Facing while waiting, 4096 = full turn (+0xa00).
    pub direction: i16,
    /// +0x1e (runtime +0xa80; use not traced).
    pub extra: i16,
    /// +0x20 (runtime +0xb00, then +0xc08).
    pub flags: u32,
    /// +0x24: the area (runtime +0x800 = 0x40000000 >> area).
    pub area: u32,
}

impl Waypoint {
    /// The point to look at, when the flags use it.
    pub fn look_at(&self) -> Option<[f32; 3]> {
        (self.flags & 0x30F != 0).then_some(self.look)
    }
}

#[derive(Clone, Debug)]
pub struct Area {
    pub grid_origin: [i32; 3],
    pub cell_size: [i32; 3],
    pub groups: Vec<Group>,
    /// Navigation nodes (area +04 count, +0C table).
    pub nav: Vec<NavNode>,
}

/// One 0x40-byte navigation node: a walkable rectangle.
/// `f32 x, z, y, half_x, half_z; u16 flags; u8 links[6] @+16; ...`.
/// Node lookup: `FUN_001451B8`; containment `FUN_00144830` (inside the
/// rectangle and -1000 <= dy < 2000, or -10 <= dy < 500 with flag 0x20).
/// The area also holds precomputed shortest-path matrices (+18, +1C, +24..),
/// which are not decoded; paths can be recomputed from the links.
#[derive(Clone, Debug)]
pub struct NavNode {
    pub position: [f32; 3],
    pub half: [f32; 2],
    pub flags: u16,
    pub links: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Group {
    /// Cell centre, x y z (already reordered from the file's x z y).
    pub origin: [i32; 3],
    pub cell: u16,
    pub walls: Vec<Wall>,
    pub floors: Vec<Floor>,
    pub walls2: Vec<Wall>,
    pub floors2: Vec<Floor>,
}

#[derive(Clone, Debug)]
pub struct Wall {
    pub flags: u32,
    /// World-space points (x, y, z) and the wall's height at each.
    pub points: Vec<([f32; 3], f32)>,
}

#[derive(Clone, Debug)]
pub struct Floor {
    pub flags: u32,
    pub polygons: Vec<Polygon>,
}

#[derive(Clone, Debug)]
pub struct Polygon {
    pub vertices: Vec<[f32; 3]>,
    pub normal: [f32; 3],
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn get<const N: usize>(&self, o: usize) -> Result<[u8; N]> {
        self.0
            .get(o..o + N)
            .map(|b| b.try_into().unwrap())
            .ok_or_else(|| crate::Error::Format(format!("hzx: read past end at {o:#x}")))
    }
    fn u16(&self, o: usize) -> Result<u16> {
        self.get(o).map(u16::from_le_bytes)
    }
    fn i16(&self, o: usize) -> Result<i16> {
        self.get(o).map(i16::from_le_bytes)
    }
    fn u32(&self, o: usize) -> Result<u32> {
        self.get(o).map(u32::from_le_bytes)
    }
    fn i32(&self, o: usize) -> Result<i32> {
        self.get(o).map(i32::from_le_bytes)
    }
}

pub fn parse(data: &[u8]) -> Result<Hzx> {
    let r = R(data);
    let n_areas = r.u16(2)? as usize;
    let areas_off = r.u32(0x1C)? as usize;
    if areas_off + n_areas * 0x5C > data.len() {
        return format_err("hzx: area table out of range");
    }
    let mut areas = Vec::with_capacity(n_areas);
    let mut traps = Vec::new();
    for a in 0..n_areas {
        let ao = areas_off + a * 0x5C;
        let n_groups = r.u16(ao + 0x50)? as usize;
        let groups_off = r.u32(ao + 0x54)? as usize;
        let mut groups = Vec::with_capacity(n_groups);
        for g in 0..n_groups {
            let go = groups_off + g * 0x34;
            let grp = parse_group(&r, go)?;
            let (count, list) = (r.u16(go + 0x1C)? as usize, r.u32(go + 0x30)? as usize);
            for i in 0..count {
                let p = list + i * 20;
                let v = |k: usize| r.i16(p + k * 2);
                let o = grp.origin.map(|c| c as f32);
                let t = TrapBox {
                    name: r.u32(p + 16)?,
                    min: [f32::from(v(0)?) + o[0], f32::from(v(1)?) + o[1], f32::from(v(2)?) + o[2]],
                    max: [f32::from(v(4)?) + o[0], f32::from(v(5)?) + o[1], f32::from(v(6)?) + o[2]],
                };
                if !traps.contains(&t) {
                    traps.push(t);
                }
            }
            groups.push(grp);
        }
        let (n_nav, nav_off) = (r.u32(ao + 4)? as usize, r.u32(ao + 0xC)? as usize);
        let mut nav = Vec::with_capacity(n_nav);
        if nav_off != 0 {
            for i in 0..n_nav {
                let p = nav_off + i * 0x40;
                let f = |o: usize| r.u32(p + o).map(f32::from_bits);
                let links = (0..6)
                    .map(|k| r.get::<1>(p + 0x16 + k).map(|b| b[0]))
                    .collect::<Result<Vec<u8>>>()?
                    .into_iter()
                    .take_while(|b| *b != 0xFF)
                    .collect();
                nav.push(NavNode {
                    position: [f(0)?, f(8)?, f(4)?],
                    half: [f(0xC)?, f(0x10)?],
                    flags: r.u16(p + 0x14)?,
                    links,
                });
            }
        }
        areas.push(Area {
            grid_origin: [r.i32(ao + 0x34)?, r.i32(ao + 0x38)?, r.i32(ao + 0x3C)?],
            cell_size: [r.i32(ao + 0x40)?, r.i32(ao + 0x44)?, r.i32(ao + 0x48)?],
            groups,
            nav,
        });
    }
    let mut routes = Vec::new();
    let (n_routes, routes_off) = (r.u16(4)? as usize, r.u32(8)? as usize);
    for i in 0..n_routes {
        let (count, at) = (r.u32(routes_off + i * 12)? as usize, r.u32(routes_off + i * 12 + 4)? as usize);
        let mut points = Vec::with_capacity(count);
        for k in 0..count {
            let p = at + k * 40;
            let f = |o: usize| r.u32(p + o).map(f32::from_bits);
            let h = |n: usize| r.i16(p + 16 + n * 2);
            points.push(Waypoint {
                position: [f(0)?, f(8)?, f(4)?],
                look: [f(12)?, f(20)?, f(16)?],
                action: h(4)?,
                wait: h(5)?,
                direction: h(6)?,
                extra: h(7)?,
                flags: r.u32(p + 0x20)?,
                area: r.u32(p + 0x24)?,
            });
        }
        routes.push(Route { points });
    }
    Ok(Hzx { areas, routes, traps })
}

fn parse_group(r: &R, go: usize) -> Result<Group> {
    let origin = [r.i32(go)?, r.i32(go + 8)?, r.i32(go + 4)?];
    let mut grp = Group { origin, cell: r.u16(go + 0x1E)?, ..Default::default() };
    for kind in 0..4 {
        let count = r.u16(go + 0x14 + kind * 2)? as usize;
        let list = r.u32(go + 0x20 + kind * 4)? as usize;
        for e in 0..count {
            let eo = list + e * 0x20;
            let n = r.i16(eo + 6)?.max(0) as usize;
            let poly = r.i16(eo + 0xE)?.max(1) as usize;
            let flags = r.u32(eo + 0x14)?;
            let vo = r.u32(eo + 0x10)? as usize;
            let mut verts = Vec::with_capacity(n);
            for i in 0..n {
                let p = vo + i * 8;
                let v = [r.i16(p)?, r.i16(p + 2)?, r.i16(p + 4)?, r.i16(p + 6)?];
                let world = [0, 1, 2].map(|k| (i32::from(v[k]) + origin[k]) as f32);
                verts.push((world, v[3]));
            }
            match kind {
                0 | 2 => {
                    let wall = Wall { flags, points: verts.iter().map(|&(p, h)| (p, f32::from(h))).collect() };
                    if kind == 0 { grp.walls.push(wall) } else { grp.walls2.push(wall) }
                }
                _ => {
                    let polygons = verts
                        .chunks(poly)
                        .map(|c| Polygon {
                            vertices: c.iter().map(|v| v.0).collect(),
                            normal: std::array::from_fn(|k| c.get(k).map_or(0.0, |v| f32::from(v.1) / 32000.0)),
                        })
                        .collect();
                    let floor = Floor { flags, polygons };
                    if kind == 1 { grp.floors.push(floor) } else { grp.floors2.push(floor) }
                }
            }
        }
    }
    Ok(grp)
}
