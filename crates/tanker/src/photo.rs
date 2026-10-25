//! The digital camera in the holds (w04c): photographing Metal Gear RAY.
//!
//! The stage script registers photo targets (`command #661bee`, quads on
//! RAY) and a proc to run after each photo (`#8b12d1`). On every shot the
//! game grades each target (`FUN_0031cc18` in the w04c resident image) and
//! calls the target's proc with the grade; the procs turn grades into a
//! score code (10 front, 20 front-right, 30 front-left, 40 the "MARINES"
//! lettering, higher codes are poorer shots, 1000 nothing) and keep the
//! best one in As16:0x50, which the after-photo proc stores in the album
//! slot As16:0x52[Au32:0x4c]. Sending the album from the terminal plays
//! Otacon's review (CODEC.DAT record 125), which sets the flags the stage
//! checks to finish the chapter.
//!
//! C toggles the camera view; A/D and W/S aim, Q/E zoom, F takes a photo.

use bevy::prelude::*;

use crate::body::Body;
use crate::gcl::{Banks, PhotoTarget};
use crate::player::Player;
use crate::{Game, LevelRoots, SCALE, Session};

/// Album: six photos (As16) from bank A 0x52; 10000 is an empty slot.
const ALBUM: usize = 0x52;
const SLOTS: usize = 6;
const EMPTY: i16 = 10000;
/// Best score of the photo being graded (As16:0x50; 1000 = nothing).
const BEST: usize = 0x50;
/// Album slot the camera saves into (Au32:0x4c).
const SLOT: u16 = 0x4C;
/// Snake's position as the scripts read it (Bu32 0xe0 / 0xe4 / 0xe8).
const SNAKE_POS: u16 = 0xE0;
/// The camera's line-of-sight test (`FUN_0014f230(.., 0xf, 0x10004,
/// 0x10004)`): walls with any of these flag bits, and floors with any of
/// those, do not block.
const SIGHT_WALLS: u32 = 0x10004 | 0x40000 | 0x80000;
const SIGHT_FLOORS: u32 = 0x10004 | 0x40000;
/// Eye height in first person (mm above the feet).
const EYE: f32 = 1550.0;

/// A new game fills the album with empty slots (`init` script's
/// `#c74f97 As16:0x52[0] 10000 ...`); that script is not run here.
pub fn init_slots(banks: &mut Banks) {
    if (0..SLOTS).all(|i| slot(banks, i) == 0) {
        for i in 0..SLOTS {
            set_slot(banks, i, EMPTY);
        }
    }
}

fn slot(banks: &Banks, i: usize) -> i16 {
    let o = ALBUM + i * 2;
    i16::from_le_bytes([banks.a[o], banks.a[o + 1]])
}

fn set_slot(banks: &mut Banks, i: usize, v: i16) {
    let o = ALBUM + i * 2;
    banks.a[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

/// What a score code shows, from Otacon's comments in record 125.
pub fn describe(code: i16) -> &'static str {
    match code {
        10 => "RAY from the front",
        20 => "RAY from the front-right",
        30 => "RAY from the front-left",
        40 => "the \"MARINES\" lettering",
        50..=80 => "RAY, but too far away",
        90..=120 => "RAY, but something is in the way",
        130 => "a Marine",
        170 => "the commandant",
        EMPTY => "(empty)",
        _ => "nothing useful",
    }
}

/// Angle of a vector in 4096ths: yaw `atan2(x, z)` (`FUN_001411f8`) and
/// pitch `atan2(y, |xz|)` (`FUN_00141258`).
fn yaw(v: Vec3) -> i32 {
    ((v.x.atan2(v.z) * 2048.0 / std::f32::consts::PI + 0.5) as i32) & 0xFFF
}

fn pitch(v: Vec3) -> i32 {
    yaw(Vec3::new(v.y, 0.0, Vec2::new(v.x, v.z).length()))
}

/// `FUN_001413d0`: difference of two angles, 0..2048.
fn angle_diff(a: i32, b: i32) -> i32 {
    let d = (b - a) & 0xFFF;
    if d > 0x800 { 0x1000 - d } else { d }
}

/// `FUN_0031c968`: is `p` inside the quad (either winding)?
fn inside(p: Vec2, q: &[Vec2; 4]) -> bool {
    let side = |a: Vec2, b: Vec2| (p.y - a.y) * (b.x - a.x) - (p.x - a.x) * (b.y - a.y);
    let s: Vec<f32> = (0..4).map(|i| side(q[i], q[(i + 1) % 4])).collect();
    s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
}

/// `FUN_0031c8f0`: area of the projected quad.
fn area(q: &[Vec2; 4]) -> f32 {
    let tri = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    ((tri(q[0], q[1], q[2]) + tri(q[0], q[2], q[3])) * 0.5).abs()
}

/// Grades how a photo taken from `eye` looking at `look` (mm), with
/// `clip` = projection * view, shows a target (`FUN_0031cc18`):
/// 4 whole, unblocked and covering `-f` percent of the frame; 3 whole and
/// unblocked but smaller; 2 whole but partly blocked; 1 cut off but
/// centred (or so close it fills the frame); 0 missed. `blocked` tells
/// whether the line from the eye to a point hits the stage.
pub fn grade(t: &PhotoTarget, eye: Vec3, look: Vec3, clip: Mat4, blocked: impl Fn(Vec3) -> bool) -> i32 {
    let c = t.corners.map(Vec3::from);
    let normal = (c[2] - c[1]).cross(c[1] - c[0]);
    let to_eye = eye - look;
    if to_eye.dot(normal) <= 0.0
        || angle_diff(yaw(to_eye), yaw(normal)) > t.yaw
        || angle_diff(pitch(to_eye), pitch(normal)) > t.pitch
    {
        return 0;
    }
    let project = |p: Vec3| {
        let v = clip * p.extend(1.0);
        (Vec2::new(v.x / v.w, v.y / v.w), v.w > 0.0)
    };
    let mut screen = [Vec2::ZERO; 4];
    let (mut off, mut hidden) = (0u32, 0u32);
    for (i, p) in c.iter().enumerate() {
        let (s, front) = project(*p);
        screen[i] = s;
        if !front || s.x.abs() >= 1.0 || s.y.abs() >= 1.0 {
            off |= 1 << i;
        } else if blocked(*p) {
            hidden |= 1 << i;
        }
    }
    let (centre, centre_front) = project((c[0] + c[1] + c[2] + c[3]) * 0.25);
    if off == 0xF && [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].iter().all(|&(x, y)| inside(Vec2::new(x, y), &screen)) {
        return 1;
    }
    if off == 0 {
        if hidden != 0 {
            2
        } else if area(&screen) * 0.25 * 100.0 >= t.fill {
            4
        } else {
            3
        }
    } else if centre_front && centre.x.abs() <= 0.3 && centre.y.abs() <= 0.3 {
        1
    } else {
        0
    }
}

#[derive(Resource)]
pub struct PhotoMode {
    pub active: bool,
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
    /// Last result shown on screen, and for how long more.
    pub message: String,
    pub message_for: f32,
    /// White flash after the shutter.
    pub flash: f32,
}

impl Default for PhotoMode {
    fn default() -> Self {
        Self { active: false, yaw: 0.0, pitch: 0.0, fov: 0.9, message: String::new(), message_for: 0.0, flash: 0.0 }
    }
}

/// `--shots`: at each time (seconds) stand Snake at a point, aim the camera
/// (degrees) and half a second later take a photo (or, with a seventh value
/// of 0, only move him there facing `yaw`); for automated tests.
#[derive(Resource)]
pub struct AutoShots(pub Vec<AutoShot>, pub Option<f32>);

#[derive(Clone, Copy)]
pub struct AutoShot {
    at: f32,
    pos: Vec3,
    yaw: f32,
    pitch: f32,
    shoot: bool,
}

pub fn parse_shots(s: &str) -> Vec<AutoShot> {
    s.split(';')
        .filter_map(|shot| {
            let v: Vec<f32> = shot.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (v.len() >= 6).then(|| AutoShot {
                at: v[0],
                pos: Vec3::new(v[1], v[2], v[3]),
                yaw: v[4].to_radians(),
                pitch: v[5].to_radians(),
                shoot: v.get(6).is_none_or(|x| *x != 0.0),
            })
        })
        .collect()
}

#[derive(Component)]
pub struct PhotoHud;

pub fn setup_hud(mut commands: Commands) {
    commands.spawn((
        PhotoHud,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(20.0), ..default() },
        TextColor(Color::srgb(0.95, 0.95, 0.8)),
        Node { position_type: PositionType::Absolute, left: percent(3), bottom: percent(4), ..default() },
    ));
}

fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos())
}

#[allow(clippy::too_many_arguments)]
pub fn control(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    auto: Option<ResMut<AutoShots>>,
    session: Res<Session>,
    mut mode: ResMut<PhotoMode>,
    mut game: ResMut<Game>,
    mut player: Query<(&mut Body, &mut Player)>,
    windows: Query<&Window>,
    level: LevelRoots,
    bullets: Query<Entity, With<crate::bullet::Bullet>>,
) {
    let dt = time.delta_secs().min(0.05);
    mode.message_for -= dt;
    mode.flash = (mode.flash - dt * 3.0).max(0.0);
    let Ok((mut body, mut p)) = player.single_mut() else { return };
    let mut shoot = keys.just_pressed(KeyCode::KeyF);
    if let Some(mut a) = auto {
        let now = time.elapsed_secs();
        if a.1.is_some_and(|t| now >= t) {
            a.1 = None;
            shoot = true;
        }
        if let Some(&s) = a.0.first()
            && now >= s.at
        {
            a.0.remove(0);
            body.pos = s.pos;
            body.yaw = s.yaw;
            mode.active = s.shoot;
            mode.yaw = s.yaw;
            mode.pitch = s.pitch;
            if s.shoot {
                a.1 = Some(now + 0.5);
            }
        }
    }
    if game.photos.is_empty() || p.dead() {
        mode.active = false;
        return;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        mode.active = !mode.active;
        mode.yaw = body.yaw;
        mode.pitch = 0.0;
    }
    if !mode.active {
        return;
    }
    // Snake holds still while looking through the camera.
    p.busy = p.busy.max(dt * 1.5);
    let turn = 1.2 * dt * mode.fov;
    if keys.pressed(KeyCode::KeyA) {
        mode.yaw += turn;
    }
    if keys.pressed(KeyCode::KeyD) {
        mode.yaw -= turn;
    }
    if keys.pressed(KeyCode::KeyW) {
        mode.pitch = (mode.pitch + turn).min(1.4);
    }
    if keys.pressed(KeyCode::KeyS) {
        mode.pitch = (mode.pitch - turn).max(-1.4);
    }
    if keys.pressed(KeyCode::KeyQ) {
        mode.fov = (mode.fov * (1.0 - dt)).max(0.15);
    }
    if keys.pressed(KeyCode::KeyE) {
        mode.fov = (mode.fov * (1.0 + dt)).min(1.1);
    }
    body.yaw = mode.yaw;
    if !shoot {
        return;
    }

    // The shutter: grade every target from this view.
    let eye = body.pos + Vec3::Y * EYE;
    let dir = look_dir(mode.yaw, mode.pitch);
    let look = eye + dir * 1000.0;
    let aspect = windows.single().map_or(4.0 / 3.0, |w| w.width() / w.height().max(1.0));
    let clip = Mat4::perspective_rh(mode.fov, aspect, 10.0, 1.0e6) * Mat4::look_to_rh(eye, dir, Vec3::Y);
    let grades: Vec<(u32, i32)> = {
        let col = game.collision.as_ref();
        let blocked = |pt: Vec3| col.and_then(|c| c.ray_masked(eye, pt, SIGHT_WALLS, SIGHT_FLOORS)).is_some_and(|t| t < 0.97);
        game.photos.iter().map(|t| (t.proc, grade(t, eye, look, clip, blocked))).collect()
    };
    let done = game.photo_done;
    let banks = &mut game.banks;
    let free = (0..SLOTS).find(|&i| slot(banks, i) == EMPTY);
    let into = free.unwrap_or_else(|| (0..SLOTS).max_by_key(|&i| slot(banks, i)).unwrap_or(0));
    banks.set_a32(SLOT, into as i32);
    banks.a[BEST..BEST + 2].copy_from_slice(&1000i16.to_le_bytes());
    for (i, v) in [body.pos.x, body.pos.y + p.height() * 0.5, body.pos.z].into_iter().enumerate() {
        let o = SNAKE_POS as usize + i * 4;
        banks.b[o..o + 4].copy_from_slice(&(v as i32).to_le_bytes());
    }
    info!("photo into slot {into}: grades {:?}", grades.iter().map(|g| g.1).collect::<Vec<_>>());
    crate::script_event(
        &mut commands,
        &session,
        &mut game,
        |vm| {
            for (proc, g) in &grades {
                vm.call_proc(*proc, &[*g]);
            }
            if let Some(d) = done {
                vm.call_proc(d, &[]);
            }
        },
        &level,
        &bullets,
    );
    let code = slot(&game.banks, into);
    info!("photo {into}: code {code} ({})", describe(code));
    mode.message = format!("Photo {} saved: {}", into + 1, describe(code));
    mode.message_for = 4.0;
    mode.flash = 1.0;
}

/// First-person view while the camera is up (runs after the follow camera).
pub fn view(
    mode: Res<PhotoMode>,
    mut player: Query<(&Body, &mut Visibility), With<Player>>,
    mut cam: Query<(&mut Transform, &mut Projection), With<crate::player::FollowCam>>,
) {
    // Snake is not drawn while looking through the camera.
    if let Ok((_, mut v)) = player.single_mut() {
        let want = if mode.active { Visibility::Hidden } else { Visibility::Inherited };
        if *v != want {
            *v = want;
        }
    }
    let Ok((mut tf, mut proj)) = cam.single_mut() else { return };
    let Projection::Perspective(persp) = proj.as_mut() else { return };
    let mode = mode.into_inner();
    // Out of the camera view the follow camera sets the field of view.
    if !mode.active {
        return;
    }
    let Ok((b, _)) = player.single() else { return };
    let eye = (b.pos + Vec3::Y * EYE) * SCALE;
    tf.translation = eye;
    tf.look_to(look_dir(mode.yaw, mode.pitch), Vec3::Y);
    persp.fov = mode.fov;
}

pub fn hud(mode: Res<PhotoMode>, game: Res<Game>, mut q: Query<&mut Text, With<PhotoHud>>) {
    let Ok(mut t) = q.single_mut() else { return };
    let mut s = String::new();
    // Only while the camera view is up (the game shows nothing for it
    // otherwise).
    if mode.active && !game.photos.is_empty() {
        let album: Vec<String> = (0..SLOTS)
            .map(|i| match slot(&game.banks, i) {
                EMPTY => "-".to_string(),
                c => c.to_string(),
            })
            .collect();
        s = format!("CAMERA  album: {}", album.join(" "));
        s += &format!("\nA/D W/S aim  Q/E zoom ({:.0} deg)  F shoot  C close", mode.fov.to_degrees());
    }
    if mode.message_for > 0.0 {
        s += &format!("\n{}", mode.message);
    }
    if **t != s {
        **t = s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn front() -> PhotoTarget {
        // w04c's front target (`#661bee -p ... -f 5`).
        PhotoTarget {
            corners: [[-2500.0, -6000.0, -15000.0], [2500.0, -6000.0, -15000.0], [2500.0, -11250.0, -15000.0], [-2500.0, -11250.0, -15000.0]],
            fill: 5.0,
            proc: 0,
            pitch: 640,
            yaw: 320,
        }
    }

    fn shot(eye: Vec3, at: Vec3, fov: f32) -> i32 {
        let dir = (at - eye).normalize();
        let clip = Mat4::perspective_rh(fov, 4.0 / 3.0, 10.0, 1.0e6) * Mat4::look_to_rh(eye, dir, Vec3::Y);
        grade(&front(), eye, eye + dir * 1000.0, clip, |_| false)
    }

    #[test]
    fn grades() {
        let centre = Vec3::new(0.0, -8625.0, -15000.0);
        // Straight on from 12 m: whole and big enough.
        assert_eq!(shot(Vec3::new(0.0, -8625.0, -3000.0), centre, 0.9), 4);
        // From very far: whole but small.
        assert_eq!(shot(Vec3::new(0.0, -8625.0, 200000.0), centre, 0.9), 3);
        // From behind: missed.
        assert_eq!(shot(Vec3::new(0.0, -8625.0, -30000.0), centre, 0.9), 0);
        // Right up close: fills the frame.
        assert_eq!(shot(Vec3::new(0.0, -8625.0, -14000.0), centre, 0.9), 1);
        // Looking away: missed.
        assert_eq!(shot(Vec3::new(0.0, -8625.0, -3000.0), Vec3::new(20000.0, -8625.0, -3000.0), 0.9), 0);
    }
}
