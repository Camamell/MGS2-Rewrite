//! `.sar` motion sound events: for each motion of the `.mar` with the same
//! name (by index), sounds keyed to points in the motion: Snake's and the
//! soldiers' footsteps, cloth and gear noises. Read by `FUN_003b3568` /
//! `FUN_003b33b0`, played by `FUN_003b3610` (or the character's own
//! handler, which first maps the sound through the stage's `#b44bbb`
//! tables by floor material).
//!
//! ```text
//! +00 u32 ?            +04 u32 motion count   +08 u32 data size
//! +20 count x { u32 motion name, u32 data offset, u32 size, u32 stream }
//! data (after the entries), per motion:
//!   u32 n, n x { u16 sound, u8 mode, u8 joint, u32 ? }   (the sound table)
//!   at `stream`: { u16 time, u8 length, u8 sound index, ... } until time 0xffff
//! ```
//!
//! Times are in the motion's frames times its step (5 per 60 Hz frame).

use crate::{Result, format_err};

#[derive(Clone, Copy, Debug)]
pub struct Event {
    /// Motion frame x step at which the sound plays.
    pub time: u16,
    pub sound: u16,
    /// Driver play mode (`FUN_00114ce8`'s third argument, 0-4 for modes
    /// 0/3, 1, 2, 4, 5).
    pub mode: u8,
    /// The joint the sound is placed at.
    pub joint: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Sar {
    /// Events of each motion, by motion index, in time order.
    pub motions: Vec<Vec<Event>>,
}

pub fn parse(d: &[u8]) -> Result<Sar> {
    let u32_at = |o: usize| d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let u16_at = |o: usize| d.get(o..o + 2).map(|b| u16::from_le_bytes(b.try_into().unwrap()));
    let Some(count) = u32_at(4).map(|c| c as usize).filter(|c| *c < 4096) else {
        return format_err("sar: bad header");
    };
    let base = 0x20 + count * 16;
    if base > d.len() {
        return format_err("sar: truncated entries");
    }
    let mut motions = Vec::with_capacity(count);
    for m in 0..count {
        let e = 0x20 + m * 16;
        let (off, size, stream) = (u32_at(e + 4).unwrap() as usize, u32_at(e + 8).unwrap() as usize, u32_at(e + 12).unwrap() as usize);
        let mut events = Vec::new();
        if size > 0 {
            let block = base + off;
            let n = u32_at(block).unwrap_or(0) as usize;
            let mut p = block + stream;
            while let (Some(time), Some(&len), Some(&index)) = (u16_at(p), d.get(p + 2), d.get(p + 3)) {
                if time == 0xFFFF || len == 0 || p >= block + size {
                    break;
                }
                let s = block + 4 + index as usize * 8;
                if (index as usize) < n
                    && let (Some(sound), Some(&mode), Some(&joint)) = (u16_at(s), d.get(s + 2), d.get(s + 3))
                {
                    events.push(Event { time, sound, mode, joint });
                }
                p += len as usize;
            }
        }
        motions.push(events);
    }
    Ok(Sar { motions })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-motion archive: motion 0 without events, motion 1 with a
    /// two-sound table and two events.
    fn sample() -> Vec<u8> {
        let mut d = vec![0u8; 0x20 + 2 * 16];
        d[4..8].copy_from_slice(&2u32.to_le_bytes());
        // Motion 1: data offset 0, size 0x28, stream at 0x14.
        let e = 0x20 + 16;
        d[e + 4..e + 8].copy_from_slice(&0u32.to_le_bytes());
        d[e + 8..e + 12].copy_from_slice(&0x28u32.to_le_bytes());
        d[e + 12..e + 16].copy_from_slice(&0x14u32.to_le_bytes());
        let mut block = vec![0u8; 0x28];
        block[0..4].copy_from_slice(&2u32.to_le_bytes());
        // Entry 0: sound 3, mode 0, joint 16; entry 1: sound 2, joint 20.
        block[4..8].copy_from_slice(&[3, 0, 0, 16]);
        block[12..16].copy_from_slice(&[2, 0, 0, 20]);
        // Events: time 5 -> entry 1, time 135 -> entry 0, end.
        block[0x14..0x18].copy_from_slice(&[5, 0, 4, 1]);
        block[0x18..0x1C].copy_from_slice(&[135, 0, 4, 0]);
        block[0x1C..0x20].copy_from_slice(&[0xFF, 0xFF, 0, 0]);
        d.extend(block);
        d
    }

    #[test]
    fn events_by_motion() {
        let s = parse(&sample()).unwrap();
        assert_eq!(s.motions.len(), 2);
        assert!(s.motions[0].is_empty());
        let ev = &s.motions[1];
        assert_eq!(ev.len(), 2);
        assert_eq!((ev[0].time, ev[0].sound, ev[0].joint), (5, 2, 20));
        assert_eq!((ev[1].time, ev[1].sound, ev[1].joint), (135, 3, 16));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(&[0xFF; 8]).is_err());
    }
}
