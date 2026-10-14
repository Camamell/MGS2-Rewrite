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
//! moved by the actor later (the commandant's walk) and are left out here.
//!
//! Drawn standing with the first `ushold` motion, with their variants'
//! parts and head accessories; slung rifles and the crowd's reactions are
//! not done yet.

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
    let placed = placed(s);
    list.chunks_exact(RECORD)
        .filter_map(|r| {
            let v = |i: usize| r[i].int().unwrap_or(0);
            if !matches!(r[0], Val::Name(_) | Val::Int(_)) {
                return None;
            }
            let mut pos = Vec3::new(v(1) as f32, v(2) as f32, v(3) as f32);
            let mut yaw = v(5);
            // A member the `-c` actor stands somewhere (the commandant).
            if let Some((at, dir)) = placed.iter().find(|p| p.0 == v(0)).map(|p| (p.1, p.2)) {
                (pos, yaw) = (at, dir);
            }
            (pos.y != 0.0).then(|| Member {
                pos,
                yaw: yaw as f32 / 4096.0 * std::f32::consts::TAU,
                variant: v(7).rem_euclid(22),
                idle: v(9).rem_euclid(6),
                flags: v(10),
            })
        })
        .collect()
}

/// `-c x y z dir member actor` (repeated): members a speaker actor
/// (`FUN_00230938`, named `actor`) stands at `x y z` facing `dir` (w04c:
/// the commandant and an officer, placed at the origin in `-a`). Their
/// movement during the speech is not ported. Returns (member name, place,
/// facing).
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
/// the origin in w04c) is left out: he is not a patrol.
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

/// The `-e` procs (by event: 0 a button, 1 and 2 Snake near the `-p`
/// points, 3 a Marine heard something, 4 a Marine saw Snake, ...) and the
/// sight `-t near far`.
pub fn events(s: &Spawn) -> (Vec<Option<u32>>, (f32, f32)) {
    let procs = s
        .opt(b'e')
        .unwrap_or(&[])
        .iter()
        .map(|v| match v {
            Val::Proc(p) => Some(*p),
            _ => None,
        })
        .collect();
    let t = |i| s.opt_int(b't', i).map(|v| v as f32);
    (procs, (t(0).unwrap_or(2500.0), t(1).unwrap_or(7000.0)))
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
