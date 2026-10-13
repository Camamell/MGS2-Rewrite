//! Sound effects through the stage's sound driver (`music::Sound`).
//!
//! Game code queues an effect id, optionally at a position (mm); each frame
//! the queue is turned into driver commands with pan and volume from the
//! camera, as `FUN_00114ce8` does. Ids (from the Tanker resident's calls):
//! 0x52 the M9, 1 the USP, 4 an empty gun, 0x6f a door, 0x19 Snake hurt,
//! 7 / 8 footsteps, 0x16 an item taken, 0x2a the alert sting.

use std::sync::Mutex;

use bevy::prelude::*;

pub const M9: u32 = 0x52;
pub const USP: u32 = 0x01;
pub const DOOR: u32 = 0x6F;
pub const HURT: u32 = 0x19;
pub const EMPTY: u32 = 0x04;
pub const PICKUP: u32 = 0x16;
pub const RAIN: u32 = 0x1D4;
/// Guards' voices (stage effects; + the guard's voice 0..3): punched
/// (`FUN_0024b8e8`), knocked out by the last punch (`FUN_0024d020`),
/// darted and falling asleep (`FUN_00260320`, with the body's thud 0x36e).
pub const GUARD_PUNCHED: u32 = 0x114;
pub const GUARD_KNOCKED_OUT: u32 = 0x120;
pub const GUARD_ASLEEP: u32 = 0x11C;
pub const BODY_FALL: u32 = 0x36E;
const STEPS: [u32; 2] = [0x07, 0x08];

static QUEUE: Mutex<Vec<(u32, Option<Vec3>, f32)>> = Mutex::new(Vec::new());

/// Queues effect `id`, at `at` (mm) or unplaced.
pub fn play(id: u32, at: Option<Vec3>) {
    play_gain(id, at, 1.0);
}

pub fn play_gain(id: u32, at: Option<Vec3>, gain: f32) {
    // MGS2_SFX_LOG=1: log every effect (testing).
    if std::env::var_os("MGS2_SFX_LOG").is_some() {
        info!("sfx {id:#x} at {at:?}");
    }
    if let Ok(mut q) = QUEUE.lock() {
        q.push((id, at, gain));
    }
}

/// The queued effects as driver commands (`pan << 18 | volume << 12 |
/// id`), heard from `listener` (mm) facing with `right`.
pub fn drain(listener: Vec3, right: Vec3) -> Vec<u32> {
    let q = QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    q.into_iter()
        .filter_map(|(id, at, gain)| {
            let (mut vol, mut pan) = (63.0, 32.0);
            if let Some(p) = at {
                let d = p.distance(listener);
                // Full volume within 7.3 m, silent 30 m further (as the
                // distance fade of FUN_002d9698).
                if d > 7300.0 {
                    vol *= ((30000.0 - (d - 7300.0)) / 30000.0).max(0.0);
                }
                let dir = (p - listener).normalize_or_zero();
                pan += dir.dot(right) * 31.0;
            }
            let vol = (vol * gain).round().clamp(0.0, 63.0) as u32;
            (vol > 0).then(|| (pan.round().clamp(0.0, 63.0) as u32) << 18 | vol << 12 | id)
        })
        .collect()
}

/// Footsteps from Snake's travel: a step every 0.75 m running, quieter and
/// every 0.6 m walking.
pub fn footsteps(time: Res<Time>, q: Query<(&crate::body::Body, &crate::player::Player)>, mut walked: Local<(f32, usize)>) {
    let Ok((body, p)) = q.single() else { return };
    if p.speed < 300.0 {
        walked.0 = 0.0;
        return;
    }
    let running = p.speed > 2000.0;
    walked.0 += p.speed * time.delta_secs();
    let stride = if running { 750.0 } else { 600.0 };
    if walked.0 >= stride {
        walked.0 -= stride;
        walked.1 ^= 1;
        play_gain(STEPS[walked.1], Some(body.pos), if running { 1.0 } else { 0.5 });
    }
}
