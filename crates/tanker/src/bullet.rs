//! Bullets, from the guard shot `FUN_002550C0` -> `FUN_0036D5B8(matrix,
//! flags, 1, 0x19, 10, 20000, 1000, ...)`: hit box 25 mm, damage 10,
//! range 20000 mm, speed 1000 mm per frame; aim jitter ±0x80 (4096 units)
//! on two axes.

use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::Game;
use crate::body::Body;
use crate::guard::{Guard, Zone};
use crate::olga::Olga;
use crate::player::{HEIGHT, Player, RADIUS};

/// Hits above this height (mm over the feet) count as head shots.
/// A dart striking someone (stage-independent effect).
const DART_HIT: u32 = 0x86;
const HEAD_HEIGHT: f32 = 1500.0;
/// Below this the legs (mm above the feet; assumed).
const HIP_HEIGHT: f32 = 850.0;
/// Further than this from the centre line, an arm (mm; assumed).
const ARM_OFFSET: f32 = 170.0;

pub const SPEED: f32 = 1000.0 * 60.0;
pub const RANGE: f32 = 20000.0;
pub const DAMAGE: i32 = 10;
/// Aim jitter, radians (0x80 of 4096 per axis).
pub const SPREAD: f32 = 0x80 as f32 / 4096.0 * std::f32::consts::TAU;

#[derive(Component)]
pub struct Bullet {
    pub pos: Vec3,
    pub dir: Vec3,
    pub travelled: f32,
    /// Snake's shot (hits guards) rather than a guard's round.
    pub dart: bool,
    /// A USP round: kills.
    pub lethal: bool,
}

/// Snake's M9 dart: straight along `dir`.
pub fn fire_dart(commands: &mut Commands, from: Vec3, dir: Vec3) {
    commands.spawn(Bullet { pos: from, dir: dir.normalize_or(Vec3::Z), travelled: 0.0, dart: true, lethal: false });
}

/// Snake's USP round: straight along `dir`, lethal.
pub fn fire_round(commands: &mut Commands, from: Vec3, dir: Vec3) {
    commands.spawn(Bullet { pos: from, dir: dir.normalize_or(Vec3::Z), travelled: 0.0, dart: true, lethal: true });
}

/// Small deterministic RNG (the game uses `FUN_0016F600`).
#[derive(Resource)]
pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> u32 {
        // xorshift32
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in [-1, 1).
    pub fn signed(&mut self) -> f32 {
        (self.next() % 65536) as f32 / 32768.0 - 1.0
    }
}

pub fn fire(commands: &mut Commands, rng: &mut Rng, from: Vec3, at: Vec3) {
    let d = (at - from).normalize_or_zero();
    let yaw = d.x.atan2(d.z) + rng.signed() * SPREAD;
    let pitch = d.y.asin() + rng.signed() * SPREAD;
    let dir = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos());
    commands.spawn(Bullet { pos: from, dir, travelled: 0.0, dart: false, lethal: false });
}

/// Where the segment `a`..`b` passes through a standing body's cylinder
/// (feet at `base`), as the hit height above the feet.
fn hits_body(a: Vec3, b: Vec3, base: Vec3, height: f32) -> Option<f32> {
    hit_point(a, b, base, height).map(|p| p.y - base.y)
}

fn hit_point(a: Vec3, b: Vec3, base: Vec3, height: f32) -> Option<Vec3> {
    (0..=8).find_map(|i| {
        let p = a.lerp(b, i as f32 / 8.0);
        let flat = Vec2::new(p.x - base.x, p.z - base.z).length();
        (flat < RADIUS && p.y > base.y && p.y < base.y + height).then_some(p)
    })
}

/// The game tests hit spheres on the skeleton (three head and neck zones,
/// one body, eight limb zones); here by height and by distance from the
/// centre line across the guard (proportions assumed).
fn zone(p: Vec3, body: &Body) -> Zone {
    let h = p.y - body.pos.y;
    let fwd = body.forward();
    let right = Vec3::new(fwd.z, 0.0, -fwd.x);
    let side = (p - body.pos).dot(right);
    let left = u8::from(side >= 0.0);
    if h > HEAD_HEIGHT {
        Zone::Head
    } else if h < HIP_HEIGHT {
        Zone::Limb(2 + left)
    } else if side.abs() > ARM_OFFSET {
        Zone::Limb(left)
    } else {
        Zone::Body
    }
}

/// Moves bullets, stops them at walls and applies hits to Snake.
pub fn fly(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut bullets: Query<(Entity, &mut Bullet)>,
    mut snake: Query<(&Body, &mut Player), Without<Guard>>,
    mut guards: Query<(&Body, &mut Guard), (Without<Player>, Without<Olga>)>,
    mut olgas: Query<(&Body, &mut Olga), (Without<Player>, Without<Guard>)>,
) {
    let dt = time.delta_secs().min(0.05);
    let Ok((sb, mut player)) = snake.single_mut() else { return };
    'bullets: for (e, mut b) in &mut bullets {
        let step = SPEED * dt;
        let next = b.pos + b.dir * step;
        // Stop at the first wall or floor on this frame's segment.
        let surface = game.collision.as_ref().and_then(|c| c.ray_normal(b.pos, next));
        let wall = surface.map(|s| s.0);
        let end = wall.map_or(next, |t| b.pos.lerp(next, t));
        // Snake's shots break the lights they pass through (`brk_spotlgt.c`).
        if b.dart
            && let Some(name) = game.brk_lights.iter().find(|l| !l.3 && segment_distance(b.pos, end, l.1) < LIGHT_HIT).map(|l| l.0)
        {
            if let Ok(mut q) = LIGHT_HITS.lock() {
                q.push(name);
            }
            commands.entity(e).despawn();
            continue 'bullets;
        }
        if b.dart {
            for (gb, mut g) in &mut guards {
                if g.dead() {
                    continue;
                }
                if let Some(p) = hit_point(b.pos, end, gb.pos, HEIGHT) {
                    if b.lethal {
                        // Hit voice (base 0x100); down for good once his
                        // life is gone, else he knows where Snake is.
                        crate::sfx::play(0x100 + g.voice(), Some(gb.pos));
                        if !g.shot(zone(p, gb)) {
                            g.last_seen = Some(sb.pos);
                            g.meter = crate::guard::METER_MAX;
                        }
                    } else {
                        let was_down = g.asleep();
                        // The dart's impact (effect 0x86, `FUN_0024f490`).
                        crate::sfx::play(DART_HIT, Some(gb.pos));
                        g.dart(zone(p, gb));
                        if !was_down && g.asleep() {
                            crate::sfx::play(crate::sfx::GUARD_ASLEEP + g.voice(), Some(gb.pos));
                        }
                    }
                    commands.entity(e).despawn();
                    continue 'bullets;
                }
            }
            for (ob, mut o) in &mut olgas {
                if let Some(p) = hit_point(b.pos, end, ob.pos, HEIGHT) {
                    use crate::olga::{DART_BODY, DART_HEAD, DART_LIMB};
                    let damage = match zone(p, ob) {
                        Zone::Head => DART_HEAD,
                        Zone::Body => DART_BODY,
                        Zone::Limb(_) => DART_LIMB,
                    };
                    // The dart's impact (0x86, also in `FUN_002680e0`).
                    crate::sfx::play(DART_HIT, Some(ob.pos));
                    if !o.down() {
                        // Her cry, one of two at random (0x1f5 / 0x1f6).
                        crate::sfx::play(if o.stamina & 1 == 0 { 0x1F5 } else { 0x1F6 }, Some(ob.pos));
                    }
                    o.hit(damage);
                    commands.entity(e).despawn();
                    continue 'bullets;
                }
            }
        } else if hits_body(b.pos, end, sb.pos, player.height()).is_some() && !player.dead() {
            player.damage(DAMAGE);
            commands.entity(e).despawn();
            continue;
        }
        // Rounds (not darts) leave a mark where they strike; see below.
        if let Some((_, n, flags)) = surface {
            // Snake's shots (bullet flag 2, darts too) run the stage's hook
            // for the struck material (`FUN_0036c630`): sound, noise, sparks.
            let material = flags >> 28;
            let sparks = b.dart && game.impact_sparks >> material & 1 != 0;
            if !b.dart || b.lethal || sparks {
                crate::scar::add(end, n, sparks, !b.dart || b.lethal);
            }
            if b.dart
                && let Some((id, noise)) = game.impact_sound(material)
            {
                crate::sfx::play(id, Some(end));
                crate::guard::noise(noise, end);
            }
        }
        let wall = wall.is_some();
        b.pos = next;
        b.travelled += step;
        if wall || b.travelled > RANGE {
            commands.entity(e).despawn();
        }
    }
}

pub fn draw(bullets: Query<&Bullet>, mut gizmos: Gizmos) {
    for b in &bullets {
        let p = b.pos * SCALE;
        let colour = if b.dart { Color::srgb(0.5, 0.9, 1.0) } else { Color::srgb(1.0, 0.9, 0.5) };
        gizmos.line(p, p - b.dir * 0.6, colour);
    }
}

/// Lights shot this frame (actor names), for `main::break_lights`.
pub static LIGHT_HITS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
/// How near a round must pass a light's centre to break it (mm; assumed).
const LIGHT_HIT: f32 = 250.0;

fn segment_distance(a: Vec3, b: Vec3, p: Vec3) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}
