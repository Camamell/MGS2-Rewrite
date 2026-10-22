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

/// A dart striking someone (stage-independent effect).
const DART_HIT: u32 = 0x86;

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

/// A soldier's hit zone (`FUN_0025b608`: `FUN_00107da0(zone, 0x2004011,
/// 1, 2, half extents, offset)`), riding a joint of his model
/// (`FUN_00258028` copies object `(offset - 0xc0) / 0x130`'s matrix in
/// each frame). Kind bit 0x4000 turns the offset by the joint's matrix,
/// and `FUN_00108f28` tests the box's corners through it: a box in the
/// joint's space.
struct ZoneBox {
    joint: usize,
    half: Vec3,
    offset: Vec3,
}

const fn zb(joint: usize, half: [f32; 3], offset: [f32; 3]) -> ZoneBox {
    ZoneBox { joint, half: Vec3::from_array(half), offset: Vec3::from_array(offset) }
}

/// +0x3a0: life 0 at once. The head (joint 12), a box at the chest's side
/// (joint 2) and one below the hips (joint 0); the last two get flag
/// 0x80000 under body +0xf2c bit 0x1000 (not traced; taken as clear).
const HEAD_ZONES: [ZoneBox; 3] = [zb(12, [96.0, 130.0, 96.0], [0.0, 30.0, 0.0]), zb(2, [50.0, 65.0, 165.0], [100.0, 100.0, 0.0]), zb(0, [100.0, 65.0, 150.0], [0.0, -150.0, 0.0])];
/// +0x640: the arms' upper and lower parts (joints 4, 5 and 8, 9), then the
/// legs' (13, 14 and 17, 18); zones 2n and 2n+1 wound limb n.
const LIMB_ZONES: [ZoneBox; 8] = [
    zb(4, [75.0, 150.0, 75.0], [0.0, -130.0, 0.0]),
    zb(5, [75.0, 150.0, 75.0], [0.0, -130.0, 0.0]),
    zb(8, [75.0, 150.0, 75.0], [0.0, -130.0, 0.0]),
    zb(9, [75.0, 150.0, 75.0], [0.0, -130.0, 0.0]),
    zb(13, [85.0, 200.0, 120.0], [-10.0, -200.0, -10.0]),
    zb(14, [85.0, 250.0, 120.0], [-20.0, -250.0, -20.0]),
    zb(17, [85.0, 200.0, 120.0], [-10.0, -200.0, -10.0]),
    zb(18, [85.0, 250.0, 120.0], [-20.0, -250.0, -20.0]),
];
/// +0xd40: the body (joint 2).
const BODY_ZONE: ZoneBox = zb(2, [200.0, 300.0, 200.0], [0.0, 0.0, 0.0]);

/// Where the segment `a`..`b` first meets one of a soldier's zones (0..1)
/// and the zone it strikes. A round is a swept segment (`FUN_0036c798` ->
/// `FUN_00108050`, its frame's start and end); the collision pass
/// (`FUN_00109fb0`) tests a soldier's zone groups in their order (+0x100,
/// all flagged 0x80000 and skipped; the head group; the limbs; the body)
/// with the bare segment in each zone's space (`FUN_00109b10` ->
/// `FUN_001094c8`), strikes every zone of a group it passes and stops at
/// the first group struck; the damage handler `FUN_0024a1e8` takes the
/// group's lowest zone (`FUN_0024a108`). `None` for a model without the
/// soldier's skeleton.
fn zone_hit(game: &Game, body: &Body, a: Vec3, b: Vec3) -> Option<(f32, Zone)> {
    let (t, group, i) = struck(game, body, &[&HEAD_ZONES, &LIMB_ZONES, std::slice::from_ref(&BODY_ZONE)], a, b)?;
    let zone = match group {
        0 => Zone::Head,
        1 => Zone::Limb(i as u8 / 2),
        _ => Zone::Body,
    };
    Some((t, zone))
}

/// The zone groups' test (see [`zone_hit`]): where the segment first meets
/// a zone (0..1), the first group it strikes and that group's lowest zone.
fn struck(game: &Game, body: &Body, groups: &[&[ZoneBox]], a: Vec3, b: Vec3) -> Option<(f32, usize, usize)> {
    let model = &game.data.models[body.model].1;
    if model.objects.len() < 19 {
        return None;
    }
    let motion = body.motions.and_then(|m| game.data.motions[m].1.motions.get(body.motion));
    let joints = mgs2_bevy::anim::pose_turned(model, motion, body.frame, false, body.turn);
    let owner = Quat::from_rotation_y(body.yaw);
    let test = |z: &ZoneBox| -> Option<f32> {
        let (_, rot, pos) = joints.get(z.joint)?.to_scale_rotation_translation();
        let rot = owner * rot;
        let centre = body.pos + owner * pos + rot * z.offset;
        let inv = rot.inverse();
        segment_box(inv * (a - centre), inv * (b - centre), Vec3::ZERO, z.half)
    };
    groups.iter().enumerate().find_map(|(g, zones)| {
        let hits: Vec<(usize, f32)> = zones.iter().enumerate().filter_map(|(i, z)| test(z).map(|t| (i, t))).collect();
        let t = hits.iter().map(|h| h.1).fold(f32::INFINITY, f32::min);
        hits.first().map(|&(i, _)| (t, g, i))
    })
}

/// Olga's zones (w00b `FUN_0025f510`, 0x2014011 boxes on her
/// joints): eleven 0x20-byte entries at 0x1f6090 (half extents, the joint
/// as a float, the offset, then a float whose integer >> 17 is the dart's
/// damage), in groups of 2, 8 and 1 (0x3bb778): head and heart, the limbs,
/// the body. (The damage can come from her script's options instead,
/// `FUN_00152920`; not read.)
pub struct OlgaZones {
    groups: [Vec<(ZoneBox, i32)>; 3],
}

impl OlgaZones {
    const TABLE: u32 = 0x1F_6090;
    const COUNTS: u32 = 0x3B_B778;

    pub fn read(img: &[u8]) -> Option<Self> {
        let f = |addr: u32| crate::olga::word(img, addr).map(f32::from_bits);
        let mut addr = Self::TABLE;
        let mut groups: [Vec<(ZoneBox, i32)>; 3] = Default::default();
        for (g, group) in groups.iter_mut().enumerate() {
            let n = crate::olga::word(img, Self::COUNTS + 4 * g as u32)?;
            for _ in 0..n.min(16) {
                let v = |o: u32| -> Option<[f32; 3]> { Some([f(addr + o)?, f(addr + o + 4)?, f(addr + o + 8)?]) };
                let zone = zb(f(addr + 0xc)? as usize, v(0)?, v(0x10)?);
                group.push((zone, (f(addr + 0x1c)? as u32 >> 17) as i32));
                addr += 0x20;
            }
        }
        Some(Self { groups })
    }

    /// The dart's damage where `a`..`b` strikes her.
    fn hit(&self, game: &Game, body: &Body, a: Vec3, b: Vec3) -> Option<i32> {
        let boxes: Vec<Vec<ZoneBox>> = self.groups.iter().map(|g| g.iter().map(|(z, _)| zb(z.joint, z.half.to_array(), z.offset.to_array())).collect()).collect();
        let refs: Vec<&[ZoneBox]> = boxes.iter().map(Vec::as_slice).collect();
        let (_, g, i) = struck(game, body, &refs, a, b)?;
        Some(self.groups[g][i].1)
    }
}

/// Moves bullets, stops them at walls and applies hits to Snake.
pub fn fly(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut bullets: Query<(Entity, &mut Bullet)>,
    mut snake: Query<(&Body, &mut Player), Without<Guard>>,
    mut guards: Query<(Entity, &Body, &mut Guard), (Without<Player>, Without<Olga>)>,
    mut olgas: Query<(&Body, &mut Olga), (Without<Player>, Without<Guard>)>,
    radios: Query<(&crate::body::Held, &Transform), With<crate::guard::radio::Radio>>,
) {
    let dt = time.delta_secs().min(0.05);
    let olga_zones = if olgas.is_empty() { None } else { game.data.resident.as_deref().and_then(OlgaZones::read) };
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
            && let Some(name) = game.brk_lights.iter().find(|l| !l.3 && segment_distance(b.pos, next, l.1) < LIGHT_HIT && b.pos.distance(l.1) <= b.pos.distance(end) + 500.0).map(|l| l.0)
        {
            if let Ok(mut q) = LIGHT_HITS.lock() {
                q.push(name);
            }
            commands.entity(e).despawn();
            continue 'bullets;
        }
        if b.dart {
            // A soldier's radio in the way, nearer than any body: shot to
            // pieces (`FUN_00265f50`: any of damage 0x800000004801e, darts
            // included, sets +0xb4 bit 0x10000; the effect `FUN_002b9ca8`
            // is not ported).
            // The nearest soldier struck, by his zones (the cylinder for
            // other models).
            let struck = guards
                .iter()
                .filter(|(_, _, g)| !g.dead() && !g.inactive)
                .filter_map(|(ge, gb, _)| match zone_hit(&game, gb, b.pos, end) {
                    Some((t, z)) => Some((t, z, ge)),
                    None if game.data.models[gb.model].1.objects.len() < 19 => {
                        hit_point(b.pos, end, gb.pos, HEIGHT).map(|p| ((p - b.pos).length() / (end - b.pos).length().max(1e-3), Zone::Body, ge))
                    }
                    None => None,
                })
                .min_by(|x, y| x.0.total_cmp(&y.0));
            let body_t = struck.map_or(f32::INFINITY, |s| s.0);
            let radio = radios
                .iter()
                .filter_map(|(h, t)| {
                    let g = guards.get(h.owner).ok()?.2;
                    if g.dead() || g.inactive || g.radio_shot || h.hidden {
                        return None;
                    }
                    let c = t.translation / SCALE + Vec3::Y * if g.radio_in_hand { RADIO_DROP } else { -RADIO_DROP };
                    segment_box(b.pos, end, c, RADIO_BOX).map(|t| (t, h.owner))
                })
                .min_by(|x, y| x.0.total_cmp(&y.0));
            if let Some((t, owner)) = radio
                && t <= body_t
                && let Ok((_, _, mut g)) = guards.get_mut(owner)
            {
                info!("guard {}'s radio shot", g.route);
                g.radio_shot = true;
                commands.entity(e).despawn();
                continue 'bullets;
            }
            if let Some((t, zone, ge)) = struck
                && let Ok((_, gb, mut g)) = guards.get_mut(ge)
            {
                let h = b.pos.lerp(end, t).y - gb.pos.y;
                if b.lethal {
                    // Hit voice (base 0x100); down for good once his
                    // life is gone, else he knows where Snake is.
                    crate::sfx::play(0x100 + g.voice(), Some(gb.pos));
                    info!("guard {} shot ({zone:?}, {h:.0} mm up)", g.route);
                    if !g.shot(zone) {
                        g.last_seen = Some(sb.pos);
                        g.meter = crate::guard::METER_MAX;
                    }
                } else {
                    let was_down = g.asleep();
                    // The dart's impact (effect 0x86, `FUN_0024f490`).
                    crate::sfx::play(DART_HIT, Some(gb.pos));
                    g.dart(zone);
                    info!("guard {} darted ({zone:?}, {h:.0} mm up, asleep: {})", g.route, g.asleep());
                    if !was_down && g.asleep() {
                        crate::sfx::play(crate::sfx::GUARD_ASLEEP + g.voice(), Some(gb.pos));
                    }
                }
                commands.entity(e).despawn();
                continue 'bullets;
            }
            for (ob, mut o) in &mut olgas {
                if let Some(damage) = olga_zones.as_ref().and_then(|z| z.hit(&game, ob, b.pos, end)) {
                    info!("Olga darted ({damage})");
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
            player.shot(DAMAGE, (time.elapsed_secs() * 60.0) as u64);
            commands.entity(e).despawn();
            continue;
        }
        if let Some((_, n, flags)) = surface {
            let material = flags >> 28;
            // Bullet flag 0x20 and element flag 0x1000 clear (`FUN_0036c520`)
            // run the material's `#0acf09` hook: a bullet hole. Snake's guns
            // set 0x20 except the M9 (weapon flags table 0x1f83d0: 0x4002,
            // the rest 0x2022 / 0x23); the soldiers' rounds never do.
            let scar = b.lethal && flags & 0x1000 == 0 && game.impact_scars >> material & 1 != 0;
            // Bullet flag 2 (every shot: the soldiers' `FUN_002550c0` sets it
            // unless its bit 0x10, which no caller passes) and element flag
            // 0x200 clear (`FUN_0036c630`) run the `#41cf4e` hook (sound
            // 0x27's material variant and a spark burst), then the noise:
            // 5 for flag 0x4000 (the M9), 6 for 0x2000 (Snake's other guns),
            // none for the soldiers' rounds.
            let mut sparks = false;
            if flags & 0x200 == 0 {
                if let Some((id, _)) = game.impact_sound(material) {
                    crate::sfx::play(id, Some(end));
                    sparks = true;
                }
                if b.dart {
                    crate::guard::noise(if b.lethal { 6 } else { 5 }, end);
                }
            }
            if scar || sparks {
                crate::scar::add(end, n, sparks, scar);
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

/// A soldier's radio hit box (`radio.c`, `FUN_002662d0`:
/// `FUN_00107da0(.., 0x2000011, 1, 2, (50, 100, 50), (0, -50, 0))`; the
/// y offset is +50 with the radio in a hand, `FUN_00265fe0`). Taken as
/// half extents of a box along the world axes (assumed).
const RADIO_BOX: Vec3 = Vec3::new(50.0, 100.0, 50.0);
const RADIO_DROP: f32 = 50.0;

/// Where along `a`..`b` (0..1) the segment enters the box at `c` with half
/// extents `h`, if it does.
fn segment_box(a: Vec3, b: Vec3, c: Vec3, h: Vec3) -> Option<f32> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for i in 0..3 {
        let (lo, hi) = (c[i] - h[i], c[i] + h[i]);
        if d[i].abs() < 1e-6 {
            if a[i] < lo || a[i] > hi {
                return None;
            }
            continue;
        }
        let (mut n, mut f) = ((lo - a[i]) / d[i], (hi - a[i]) / d[i]);
        if n > f {
            std::mem::swap(&mut n, &mut f);
        }
        t0 = t0.max(n);
        t1 = t1.min(f);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}
