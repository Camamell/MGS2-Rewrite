//! Sound effects through the stage's sound driver (`music::Sound`).
//!
//! Game code queues an effect id, optionally at a position (mm); each frame
//! the queue is turned into driver commands with pan and volume from the
//! camera, as `FUN_00114ce8` does. Ids (from the Tanker resident's calls):
//! 0x52 the M9, 1 the USP, 4 an empty gun, 0x6f a door, 0x19 Snake hurt,
//! 0x16 an item taken, 0x2a the alert sting.

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

/// Motion sounds (`mgs2_formats::sar`): as a body's motion passes an
/// event, its sound plays at the body, first mapped through the stage's
/// `#b44bbb` tables for the character (Snake 0, soldiers 1 + voice, as
/// `FUN_002898d0` / `FUN_00257bc0` set it) by the floor's material, as the
/// characters' event handler (`FUN_003b3768`) does. These are the game's
/// footsteps; the table also gives the noise soldiers hear (silent on most
/// floors).
pub fn motion_sounds(
    game: Res<crate::Game>,
    bodies: Query<(Entity, &crate::body::Body, Option<&crate::player::Player>, Option<&crate::guard::Guard>, Has<crate::olga::Olga>)>,
    mut last: Local<std::collections::HashMap<Entity, (usize, f32)>>,
) {
    last.retain(|e, _| bodies.contains(*e));
    for (e, body, player, guard, olga) in &bodies {
        if guard.is_some_and(|g| g.inactive) {
            continue;
        }
        // Olga sets no table: her sounds play as they are.
        let table = match (player, guard, olga) {
            (Some(_), _, _) => 0u8,
            (_, Some(g), _) => 1 + g.voice() as u8,
            (_, _, true) => 0xFF,
            _ => continue,
        };
        let Some(m) = body.motions else { continue };
        let Some((hash, mar)) = game.data.motions.get(m) else { continue };
        let Some(motion) = mar.motions.get(body.motion).filter(|mo| mo.length > 0) else { continue };
        let Some(events) = game.data.sars.iter().find(|(h, _)| h == hash).and_then(|(_, s)| s.motions.get(body.motion)) else { continue };
        let step = motion.step.max(1) as f32;
        let span = motion.length as f32 * step;
        let now = if body.once { body.frame * step } else { (body.frame * step).rem_euclid(span) };
        let prev = match last.insert(e, (body.motion, now)) {
            Some((mo, t)) if mo == body.motion => t,
            _ => -1.0,
        };
        if events.is_empty() || now == prev {
            continue;
        }
        let passed = |t: f32| if now >= prev { t > prev && t <= now } else { t > prev || t <= now };
        for ev in events.iter().filter(|ev| passed(f32::from(ev.time))) {
            let material = game.collision.as_ref().and_then(|c| c.floor_material(body.pos.x, body.pos.z, body.pos.y + 100.0)).unwrap_or(0);
            // At the event's joint (the stepping foot), as the handler
            // places it (the joint's matrix, `FUN_003b3768`).
            let model = &game.data.models[body.model].1;
            let at = mgs2_bevy::anim::pose(model, Some(motion), body.frame, false)
                .get(usize::from(ev.joint))
                .map_or(body.pos, |m| {
                    // Across the floor only (the pose carries no root height).
                    let d = Quat::from_rotation_y(body.yaw) * Vec3::from(m.translation);
                    body.pos + Vec3::new(d.x, 0.0, d.z)
                });
            if player.is_some() {
                crate::footprint::step(at, body.yaw);
            }
            // Snake's wall-keyed variants play only at a wall (+0xb64 is -1
            // otherwise; its material is not tracked, 0 assumed); soldiers
            // pass 0 (`FUN_00257bc0`).
            let wall = match player {
                Some(p) => p.wall.map(|_| 0),
                None => Some(0),
            };
            if let Some((id, noise)) = game.motion_sound(table, ev.sound, material, wall) {
                play(id, Some(at));
                crate::guard::noise(noise, at);
            }
        }
    }
}
