//! DEMO.DAT cutscenes.
//!
//! A demo is a chain of chunks `u32 kind, u32 size, u32 frame, u32 0`:
//! kind 0x10 headers, 2 event packets, 1 streamed audio, 4 subtitles, 0xf0
//! the end (its frame is the length). Frames count 300ths of a second (as
//! `sdstream.c` does), so event packets, 5 apart, come once per 60 Hz frame. An event packet
//! holds sub-records `u32 type, u32 size, u32 serial, ...` dispatched by the
//! demo player (`FUN_00216308`, table at 0x1f3198 in a cutscene stage's
//! resident image) on `type & 0x7fffffff`:
//!
//! - 2: actor messages (`FUN_00219020`). Sub-kind (+0x10) 0 sends message
//!   `1, value (+0x18)` to the model loaded by serial +0x14: models start
//!   hidden (flag 0x3000) and value 0 shows them, anything else hides them
//!   (`FUN_00217ce0`); sub-kind 1 removes the model (`FUN_002164e8`).
//! - 3: camera (`FUN_002190e0` / `FUN_00219ac8`): +0x14 f32 zoom, +0x18 f32
//!   roll (radians), +0x20 eye xyz, +0x30 target xyz (mm).
//! - 4: model load (`FUN_002163f8`): the model object is keyed by this
//!   record's serial; +0x18 name hash, +0x1c file id (type 0x0a = kms).
//! - 7: the actor stream (`FUN_00219718`): u32 count at +0x10, entries at
//!   +0x20 of `u32 actor, u16 flags, u8 ?, u8 joints, u16 data offset, u16`;
//!   the data: flag 1 a position (f32 xyz, 16 bytes), flag 2 root angles
//!   (u16 x y z in 4096ths, u16 ?), then per joint a control word (low byte:
//!   how many joints it covers, bits 0x100 << k: value k absent) and the
//!   present values: quaternion x y z w as i16 / 16383 and a translation
//!   as half floats. Absent values keep their previous value, and a control
//!   word's values serve every joint it covers. Actor ids are the serials
//!   of the model loads.

use std::collections::BTreeMap;

/// One subtitle: times (300ths of a second) and text.
#[derive(Clone, Debug)]
pub struct Line {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub frame: u32,
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub zoom: f32,
    pub roll: f32,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub serial: u32,
    pub name: u32,
    /// kms file id; its low 24 bits are the model's hash.
    pub file: u32,
    /// `.evm` skinned mesh laid over the kms (+0x10; 0 = none). Its
    /// actors' joints follow the evm skeleton, whose first 21 bones are the
    /// standard body.
    pub evm: u32,
    /// Frames it is loaded from and (if removed) until.
    pub from: u32,
    pub until: Option<u32>,
    /// Shown / hidden from these frames on (hidden until the first).
    pub shown: Vec<(u32, bool)>,
}

impl Demo {
    /// The fade colour on screen at `frame` (ticks), RGBA 0..1.
    pub fn fade_at(&self, frame: f32) -> [f32; 4] {
        let eval = |f: &Fade, start: [f32; 4], at: f32| -> [f32; 4] {
            let t = if f.frames > 0.0 { ((at - f.frame as f32) / (f.frames * 5.0)).clamp(0.0, 1.0) } else { 1.0 };
            std::array::from_fn(|i| start[i] + (f.to[i] - start[i]) * t)
        };
        // Each fade starts from its own colour or the one on screen when it
        // begins.
        let mut last: Option<(&Fade, [f32; 4])> = None;
        for f in self.fades.iter().filter(|f| f.frame as f32 <= frame) {
            let start = f.from.unwrap_or_else(|| last.map_or([0.0; 4], |(p, s)| eval(p, s, f.frame as f32)));
            last = Some((f, start));
        }
        let cur = last.map_or([0.0; 4], |(f, s)| eval(f, s, frame));
        let n = |v: f32, max: f32| (v / max).clamp(0.0, 1.0);
        [n(cur[0], 255.0), n(cur[1], 255.0), n(cur[2], 255.0), n(cur[3], 128.0)]
    }
}

impl Model {
    pub fn visible(&self, frame: f32) -> bool {
        (self.from as f32) <= frame
            && self.until.is_none_or(|u| frame < u as f32)
            && self.shown.iter().rev().find(|(f, _)| (*f as f32) <= frame).is_some_and(|(_, s)| *s)
    }
}

/// An actor's state after a packet that changed it.
#[derive(Clone, Debug)]
pub struct ActorKey {
    pub frame: u32,
    pub pos: [f32; 3],
    /// Root angles x y z in 4096ths of a turn.
    pub angles: [u16; 3],
    /// Per joint: quaternion x y z w, translation x y z.
    pub joints: Vec<[f32; 7]>,
}

#[derive(Clone, Debug, Default)]
pub struct Demo {
    pub length: u32,
    pub cameras: Vec<Camera>,
    pub models: Vec<Model>,
    pub actors: BTreeMap<u32, Vec<ActorKey>>,
    pub subtitles: Vec<Line>,
    pub fades: Vec<Fade>,
    pub fogs: Vec<DemoFog>,
    /// Ambient colour (type-6 `0x1000006`, the engine's `FUN_00125eb0`).
    pub ambients: Vec<(u32, [f32; 3])>,
    /// The light (type-6 `0x1000007`): unit direction and colour (`FUN_00125ee0`,
    /// `FUN_00125f68`). Colours count 128 as 1.0.
    pub lights: Vec<DemoLight>,
    /// Lightning bolts (type-6 `0x1002000`, `d_thunder.c`): frame and the
    /// bolt's start on screen (x, y).
    pub thunders: Vec<(u32, [f32; 2])>,
    /// Script procs the demo calls (type-6 `0x10fff01`, `cmd_10fff01` ->
    /// `FUN_00216ae8`: the proc +0x20, run only when the demo actor's `-p`
    /// lists it): frame and proc name.
    pub procs: Vec<(u32, u32)>,
}

#[derive(Clone, Copy, Debug)]
pub struct DemoLight {
    pub frame: u32,
    pub dir: [f32; 3],
    pub color: [f32; 3],
}

/// Fog from a type-6 effect `0x1000000` (`FUN_002a24c8`): RGB, near and far
/// (f32 mm) from `frame` on.
#[derive(Clone, Copy, Debug)]
pub struct DemoFog {
    pub frame: u32,
    pub color: [u8; 3],
    pub near: f32,
    pub far: f32,
}

/// A screen fade (type-6 effects `0x1000003` / `0x1000008`, the resident's
/// `d_fade_io_prog.c`): from `from` (or the colour on screen) to `to` over
/// `frames` 60 Hz frames. Colours are RGBA, alpha 0x80 = opaque.
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    pub frame: u32,
    pub from: Option<[f32; 4]>,
    pub to: [f32; 4],
    pub frames: f32,
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes(s.try_into().unwrap()))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

fn f32_at(b: &[u8], o: usize) -> Option<f32> {
    u32_at(b, o).map(f32::from_bits)
}

/// `FUN_00219a88`: half float to float, scaled by 128 (the exponent bias it
/// adds is 7 more than IEEE's), so millimetres keep their precision.
pub fn half(h: u16) -> f32 {
    let h = u32::from(h);
    let exp = if h & 0x7C00 != 0 { ((h & 0x7C00) + 0x1_DC00) * 0x2000 } else { 0 };
    f32::from_bits((h & 0x8000) << 16 | exp | (h & 0x3FF) << 13)
}

/// Length in bytes of the demo starting at `data` (up to its end chunk).
pub fn extent(data: &[u8]) -> usize {
    let mut pos = 0;
    let mut n = 0;
    while let (Some(kind), Some(size)) = (u32_at(data, pos), u32_at(data, pos + 4)) {
        let size = size as usize;
        if !(16..=0x10_0000).contains(&size) || (kind == 0x10 && n > 0) {
            break;
        }
        pos += size;
        n += usize::from(kind != 0x10);
        if kind == 0xF0 {
            break;
        }
    }
    pos.min(data.len())
}

pub fn parse(data: &[u8]) -> Demo {
    let mut demo = Demo::default();
    let mut state: BTreeMap<u32, ActorKey> = BTreeMap::new();
    let mut pos = 0;
    let mut n = 0;
    while let (Some(kind), Some(size), Some(frame)) = (u32_at(data, pos), u32_at(data, pos + 4), u32_at(data, pos + 8)) {
        let size = size as usize;
        if !(16..=0x10_0000).contains(&size) || pos + size > data.len() || (kind == 0x10 && n > 0) {
            break;
        }
        let chunk = &data[pos..pos + size];
        match kind {
            2 => packet(chunk, frame, &mut demo, &mut state),
            4 => subtitles(chunk, &mut demo.subtitles),
            0xF0 => {
                demo.length = frame;
                break;
            }
            _ => {}
        }
        demo.length = demo.length.max(frame);
        pos += size;
        n += usize::from(kind != 0x10);
    }
    demo
}

fn packet(chunk: &[u8], frame: u32, demo: &mut Demo, state: &mut BTreeMap<u32, ActorKey>) {
    let mut p = 16;
    while let (Some(t), Some(sz)) = (u32_at(chunk, p), u32_at(chunk, p + 4)) {
        let sz = sz as usize;
        if sz < 8 || p + sz > chunk.len() {
            break;
        }
        let r = &chunk[p..p + sz];
        match t & 0x7FFF_FFFF {
            2 => {
                let target = u32_at(r, 0x14).and_then(|s| demo.models.iter_mut().rev().find(|m| m.serial == s && m.until.is_none()));
                match (u32_at(r, 0x10), target) {
                    (Some(0), Some(m)) => m.shown.push((frame, u32_at(r, 0x18) == Some(0))),
                    (Some(1), Some(m)) => m.until = Some(frame),
                    _ => {}
                }
            }
            3 => {
                let v = |o: usize| f32_at(r, o).unwrap_or(0.0);
                demo.cameras.push(Camera {
                    frame,
                    eye: [v(0x20), v(0x24), v(0x28)],
                    target: [v(0x30), v(0x34), v(0x38)],
                    zoom: v(0x14),
                    roll: v(0x18),
                });
            }
            4 => {
                if let (Some(serial), Some(name), Some(file)) = (u32_at(r, 8), u32_at(r, 0x18), u32_at(r, 0x1C)) {
                    let evm = u32_at(r, 0x10).unwrap_or(0);
                    demo.models.push(Model { serial, name, file, evm, from: frame, until: None, shown: Vec::new() });
                }
            }
            7 => actors(r, frame, demo, state),
            6 => effect(r, frame, demo),
            _ => {}
        }
        p += sz;
    }
}

/// Type 6: an effect constructor (`+0x10` id in the resident's command
/// table, `+0x14` 0 = one-shot, parameters from `+0x20`). Fog, ambient, light and
/// fades are kept; durations are the difference of two frame stamps.
fn effect(r: &[u8], frame: u32, demo: &mut Demo) {
    let p = |i: usize| u32_at(r, 0x20 + i * 4).map_or(0.0, |v| v as i32 as f32);
    let rgba = |i: usize| [p(i), p(i + 1), p(i + 2), p(i + 3)];
    match u32_at(r, 0x10) {
        Some(0x0100_0000) => {
            let c = |i: usize| u32_at(r, 0x20 + i * 4).unwrap_or(0).min(255) as u8;
            let fl = |i: usize| f32_at(r, 0x20 + i * 4).unwrap_or(0.0);
            demo.fogs.push(DemoFog { frame, color: [c(0), c(1), c(2)], near: fl(4), far: fl(5) });
        }
        Some(0x0100_2000) => {
            let fl = |i: usize| f32_at(r, 0x20 + i * 4).unwrap_or(0.0);
            demo.thunders.push((frame, [fl(0), fl(1)]));
        }
        Some(0x0100_0006) => demo.ambients.push((frame, [p(0), p(1), p(2)])),
        Some(0x0100_0007) => {
            let fl = |i: usize| f32_at(r, 0x20 + i * 4).unwrap_or(0.0);
            let (x, y, z) = (fl(4), fl(5), fl(6));
            let len = (x * x + y * y + z * z).sqrt();
            if len > 0.0 {
                demo.lights.push(DemoLight { frame, dir: [x / len, y / len, z / len], color: [p(0), p(1), p(2)] });
            }
        }
        Some(0x010F_FF01) => {
            if let Some(proc) = u32_at(r, 0x20) {
                demo.procs.push((frame, proc & 0x00FF_FFFF));
            }
        }
        Some(0x0100_0003) => demo.fades.push(Fade { frame, from: None, to: rgba(0), frames: p(4) - p(5) }),
        Some(0x0100_0008) => demo.fades.push(Fade { frame, from: Some(rgba(0)), to: rgba(4), frames: p(8) - p(9) }),
        _ => {}
    }
}

fn actors(r: &[u8], frame: u32, demo: &mut Demo, state: &mut BTreeMap<u32, ActorKey>) {
    stream(r, 0x10, 0x20, false, frame, &mut demo.actors, state);
}

/// An actor stream record: u32 count at count_at, 12-byte entries from
/// entries_at; data offsets count from the record, or (fter) from the
/// end of the entries.
fn stream(r: &[u8], count_at: usize, entries_at: usize, after: bool, frame: u32, out: &mut BTreeMap<u32, Vec<ActorKey>>, state: &mut BTreeMap<u32, ActorKey>) {
    let count = (u32_at(r, count_at).unwrap_or(0) as usize).min(64);
    let base = if after { entries_at + count * 12 } else { 0 };
    for i in 0..count {
        let e = entries_at + i * 12;
        let (Some(id), Some(flags), Some(off)) = (u32_at(r, e), u16_at(r, e + 4), u16_at(r, e + 8)) else { break };
        let joints = r.get(e + 7).copied().unwrap_or(0) as usize;
        let st = state.entry(id).or_insert_with(|| ActorKey { frame, pos: [0.0; 3], angles: [0; 3], joints: Vec::new() });
        if st.joints.len() < joints {
            st.joints.resize(joints, [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        }
        let mut q = base + off as usize;
        if flags & 1 != 0 {
            st.pos = [f32_at(r, q).unwrap_or(0.0), f32_at(r, q + 4).unwrap_or(0.0), f32_at(r, q + 8).unwrap_or(0.0)];
            q += 16;
        }
        if flags & 2 != 0 {
            st.angles = [u16_at(r, q).unwrap_or(0), u16_at(r, q + 2).unwrap_or(0), u16_at(r, q + 4).unwrap_or(0)];
            q += 8;
        }
        if joints > 0 {
            let mut ctl = u16_at(r, q).unwrap_or(0x7F01);
            let mut left = ctl & 0xFF;
            for j in 0..joints {
                let mut v = q + 2;
                for k in 0..7 {
                    if ctl & (0x100 << k) == 0 {
                        let Some(raw) = u16_at(r, v) else { break };
                        v += 2;
                        st.joints[j][k] = if k < 4 { f32::from(raw as i16) / 16383.0 } else { half(raw) };
                    }
                }
                left = left.saturating_sub(1);
                if left == 0 && j + 1 < joints {
                    ctl = u16_at(r, v).unwrap_or(0x7F01);
                    left = ctl & 0xFF;
                    q = v;
                }
            }
        }
        st.frame = frame;
        out.entry(id).or_default().push(st.clone());
    }
}

/// The face streams of a voice clip (VOX.DAT / MOVIE.DAT kind-5 chunks): per
/// actor (Codec speaker), keys of joint rotations every 5 ticks. A chunk holds
/// records u32 0x1000, u32 size, u32 0, u32 count, 12-byte entries u32
/// actor, u16 flags, u8, u8 joints, u16 data offset, u16 and the data
/// (as the cutscene actor stream, offsets from the end of the entries). The
/// 32-joint actors are a speaker's face bones (21 on of the Codec heads'
/// standard skeleton).
pub fn face_stream(data: &[u8]) -> BTreeMap<u32, Vec<ActorKey>> {
    let mut out = BTreeMap::new();
    let mut state = BTreeMap::new();
    let mut pos = 0;
    let mut n = 0;
    while let (Some(kind), Some(size), Some(frame)) = (u32_at(data, pos), u32_at(data, pos + 4), u32_at(data, pos + 8)) {
        let size = size as usize;
        if !(16..=0x10_0000).contains(&size) || pos + size > data.len() || (kind == 0x10 && n > 0) {
            break;
        }
        if kind == 5 {
            let chunk = &data[pos + 16..pos + size];
            let mut p = 0;
            while let (Some(t), Some(sz)) = (u32_at(chunk, p), u32_at(chunk, p + 4)) {
                let sz = sz as usize;
                if sz < 16 || p + sz > chunk.len() {
                    break;
                }
                if t == 0x1000 {
                    stream(&chunk[p..p + sz], 0x0C, 0x10, true, frame, &mut out, &mut state);
                }
                p += sz;
            }
        }
        if kind == 0xF0 {
            break;
        }
        n += usize::from(kind != 0x10);
        pos += size;
    }
    out
}

/// Subtitle chunk: `u32 size`, then records `u32 start, u32 end, u32 0, u32
/// length (with this header), text` where `0x80 '|'` breaks the line.
fn subtitles(chunk: &[u8], out: &mut Vec<Line>) {
    let Some(payload) = u32_at(chunk, 16) else { return };
    // The size counts the records, which start after it.
    let end = (20 + payload as usize).min(chunk.len());
    let mut o = 20;
    while o + 16 <= end {
        let (Some(start), Some(stop), Some(len)) = (u32_at(chunk, o), u32_at(chunk, o + 4), u32_at(chunk, o + 12)) else { break };
        if len < 16 {
            break;
        }
        let text = &chunk[o + 16..(o + len as usize).min(end)];
        let text = text.split(|b| *b == 0).next().unwrap_or_default();
        // `0x80 '|'` breaks the line; `0x80 '#'` and two bytes is a control
        // code that is not shown.
        let mut clean = Vec::with_capacity(text.len());
        let mut i = 0;
        while i < text.len() {
            match (text[i], text.get(i + 1)) {
                (0x80, Some(b'|')) => {
                    clean.push(b'\n');
                    i += 2;
                }
                (0x80, Some(b'#')) => i += 4,
                (b'|', _) => {
                    clean.push(b'\n');
                    i += 1;
                }
                (b, _) => {
                    clean.push(b);
                    i += 1;
                }
            }
        }
        let text = String::from_utf8_lossy(&clean).into_owned();
        out.push(Line { start, end: stop, text });
        o += len as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats() {
        assert_eq!(half(0x3C00), 128.0);
        assert_eq!(half(0xC000), -256.0);
        assert_eq!(half(0), 0.0);
    }
}
