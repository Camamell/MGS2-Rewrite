//! Bodies keeping apart, and Snake's punches.

use bevy::prelude::*;

use crate::Game;
use crate::body::Body;
use crate::guard::Guard;
use crate::player::{Player, RADIUS, STEP_UP};

/// Two standing characters keep this far apart (mm).
const SPACING: f32 = RADIUS * 2.0;
/// Punch reach in front of Snake (mm) and half-angle (radians).
const PUNCH_REACH: f32 = 1000.0;
const PUNCH_ARC: f32 = 0.8;
/// Hits in a combo; the last is the knock-down blow.
const COMBO_HITS: u32 = 3;
/// Voice of a guard knocked down (base; + his voice).
const KNOCKED_DOWN: u32 = 0x104;
/// A combo breaks if the next punch comes later than this (seconds).
const COMBO_GAP: f32 = 0.8;
/// How long a punch holds Snake in place (seconds).
const PUNCH_TIME: f32 = 0.2;

/// Pushes overlapping characters apart horizontally (Snake and guards,
/// guards among themselves); sleeping guards are stepped over.
pub fn separate(
    game: Res<Game>,
    mut snake: Query<&mut Body, (With<Player>, Without<Guard>)>,
    mut guards: Query<(&mut Body, &Guard), Without<Player>>,
    marines: Query<&Body, (With<crate::crowd::Marine>, Without<Player>, Without<Guard>)>,
) {
    let mut bodies: Vec<(Vec3, bool)> = guards.iter().map(|(b, g)| (b.pos, !g.asleep() && !g.inactive)).collect();
    let snake_pos = snake.single().map(|b| b.pos).ok();
    let mut push = vec![Vec2::ZERO; bodies.len()];
    let mut snake_push = Vec2::ZERO;
    let apart = |a: Vec3, b: Vec3| -> Option<Vec2> {
        let d = Vec2::new(a.x - b.x, a.z - b.z);
        let len = d.length();
        ((a.y - b.y).abs() < 1000.0 && len < SPACING).then(|| d.normalize_or(Vec2::X) * (SPACING - len) * 0.5)
    };
    for i in 0..bodies.len() {
        if !bodies[i].1 {
            continue;
        }
        for j in i + 1..bodies.len() {
            if let (true, Some(p)) = (bodies[j].1, apart(bodies[i].0, bodies[j].0)) {
                push[i] += p;
                push[j] -= p;
            }
        }
        if let Some(s) = snake_pos
            && let Some(p) = apart(s, bodies[i].0) {
                // Snake gives way more than a planted guard.
                snake_push += p * 1.5;
                push[i] -= p * 0.5;
            }
    }
    let settle = |pos: Vec3, d: Vec2| -> Vec3 {
        let mut p = pos + Vec3::new(d.x, 0.0, d.y);
        if let Some(col) = &game.collision {
            p = col.push_out(p, RADIUS, crate::player::HEIGHT, STEP_UP);
            if col.floor_below(p.x, p.z, p.y + STEP_UP).is_none() {
                return pos;
            }
        }
        p
    };
    for ((mut b, _), (d, body)) in guards.iter_mut().zip(push.iter().zip(bodies.iter_mut())) {
        if *d != Vec2::ZERO {
            b.pos = settle(b.pos, *d);
            body.0 = b.pos;
        }
    }
    // The Marines in formation do not give way.
    if let Some(s) = snake_pos {
        for m in &marines {
            if let Some(p) = apart(s, m.pos) {
                snake_push += p * 2.0;
            }
        }
    }
    if snake_push != Vec2::ZERO
        && let Ok(mut b) = snake.single_mut() {
            b.pos = settle(b.pos, snake_push);
        }
}

/// `--punch-at`: punch at these times (seconds), for automated tests.
#[derive(Resource)]
pub struct AutoPunch(pub Vec<f32>);

/// Punch combo state.
#[derive(Resource, Default)]
pub struct Punches {
    count: u32,
    last: f32,
    target: Option<Entity>,
}

/// V punches whoever stands right in front of Snake. A guard punched
/// awake turns on Snake (discovery); blows wear down his stamina (9: a
/// punch-punch-blow combo takes 7, so two combos knock him out).
pub fn punch(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    auto: Option<ResMut<AutoPunch>>,
    mut punches: ResMut<Punches>,
    mut snake: Query<(&mut Body, &mut Player), Without<Guard>>,
    mut guards: Query<(Entity, &Body, &mut Guard), Without<Player>>,
) {
    let scripted = auto.is_some_and(|mut a| {
        let due = a.0.first().is_some_and(|t| time.elapsed_secs() >= *t);
        if due {
            a.0.remove(0);
        }
        due
    });
    if !keys.just_pressed(KeyCode::KeyV) && !scripted {
        return;
    }
    let Ok((mut sb, mut p)) = snake.single_mut() else { return };
    if p.dead() || p.aiming || p.busy > 0.0 {
        return;
    }
    sb.play_once(crate::player::motion::PUNCH);
    p.busy = PUNCH_TIME;
    let now = time.elapsed_secs();
    let fwd = sb.forward();
    let hit = guards.iter_mut().find(|(_, gb, g)| {
        let to = gb.pos - sb.pos;
        let flat = Vec2::new(to.x, to.z);
        !g.asleep()
            && !g.inactive
            && to.y.abs() < 1000.0
            && flat.length() < PUNCH_REACH
            && flat.normalize_or_zero().angle_to(Vec2::new(fwd.x, fwd.z)).abs() < PUNCH_ARC
    });
    let Some((e, gb, mut g)) = hit else {
        punches.count = 0;
        return;
    };
    if punches.target != Some(e) || now - punches.last > COMBO_GAP {
        punches.count = 0;
    }
    punches.count += 1;
    punches.last = now;
    punches.target = Some(e);
    // Punches take 1 stamina (`FUN_0024b8e8`, voice 0x114); the combo's
    // third blow knocks him down for 5 (`FUN_0024c3a8`, voice 0x104). Out
    // of stamina he is out cold (voice 0x120).
    let finisher = punches.count >= COMBO_HITS;
    g.stamina -= if finisher { 5 } else { 1 };
    if finisher {
        punches.count = 0;
    }
    if g.stamina <= 0 {
        crate::sfx::play(crate::sfx::GUARD_KNOCKED_OUT + g.voice(), Some(gb.pos));
        g.knock_out();
        punches.count = 0;
    } else {
        let voice = if finisher { KNOCKED_DOWN } else { crate::sfx::GUARD_PUNCHED };
        crate::sfx::play(voice + g.voice(), Some(gb.pos));
        g.punched(sb.pos);
    }
}
