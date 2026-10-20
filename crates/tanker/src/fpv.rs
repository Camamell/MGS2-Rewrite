//! First-person view (the game's R1 look): hold X to look through Snake's
//! eyes; A/D turn, W/S look up and down. Snake stands his ground meanwhile.
//! Not taken from the game: the turn rates and the eye height (92% of his
//! current height: standing, crouched or prone). `MGS2_FPV_TEST=1` holds it
//! on (for screenshots); `MGS2_FPV_AT=x,y,z` keeps the look on a point.

use bevy::prelude::*;

use crate::body::Body;
use crate::player::{FollowCam, Player};
use crate::SCALE;

#[derive(Resource, Default)]
pub struct FirstPerson {
    pub active: bool,
    yaw: f32,
    pitch: f32,
}

impl FirstPerson {
    /// Where he looks (unit vector).
    pub fn dir(&self) -> Vec3 {
        Vec3::new(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos())
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<FirstPerson>().add_systems(
        Update,
        (
            control.before(crate::player::control),
            view.after(crate::photo::view),
        ),
    );
}

fn control(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    photo: Res<crate::photo::PhotoMode>,
    cutscene: Res<crate::demo::Cutscene>,
    mut fp: ResMut<FirstPerson>,
    mut player: Query<(&mut Body, &mut Player)>,
) {
    let dt = time.delta_secs().min(0.05);
    let Ok((mut body, mut p)) = player.single_mut() else { return };
    let want = (keys.pressed(KeyCode::KeyX) || std::env::var_os("MGS2_FPV_TEST").is_some()) && !photo.active && !cutscene.active() && !p.dead();
    if want && !fp.active {
        fp.yaw = body.yaw;
        // MGS2_FPV_PITCH=degrees: start looking up / down (testing).
        fp.pitch = std::env::var("MGS2_FPV_PITCH").ok().and_then(|v| v.parse::<f32>().ok()).map_or(0.0, f32::to_radians);
    }
    fp.active = want;
    if !want {
        return;
    }
    p.busy = p.busy.max(dt * 1.5);
    let turn = 1.6 * dt;
    if keys.pressed(KeyCode::KeyA) {
        fp.yaw += turn;
    }
    if keys.pressed(KeyCode::KeyD) {
        fp.yaw -= turn;
    }
    if keys.pressed(KeyCode::KeyW) {
        fp.pitch = (fp.pitch + turn).min(1.3);
    }
    if keys.pressed(KeyCode::KeyS) {
        fp.pitch = (fp.pitch - turn).max(-1.3);
    }
    // MGS2_FPV_AT=x,y,z: keep looking at that point (testing).
    if let Some(at) = std::env::var("MGS2_FPV_AT").ok().and_then(|v| {
        let n: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        (n.len() == 3).then(|| Vec3::new(n[0], n[1], n[2]))
    }) {
        let d = at - (body.pos + Vec3::Y * p.height() * 0.92);
        fp.yaw = d.x.atan2(d.z);
        fp.pitch = d.y.atan2(d.xz().length());
    }
    body.yaw = fp.yaw;
}

pub fn view(
    fp: Res<FirstPerson>,
    mut player: Query<(&Body, &Player, &mut Visibility)>,
    mut cam: Query<&mut Transform, With<FollowCam>>,
    mut was: Local<bool>,
) {
    let Ok((b, p, mut vis)) = player.single_mut() else { return };
    // (The photo view sets his visibility every frame; override it after.)
    if fp.active {
        *vis = Visibility::Hidden;
    } else {
        if *was {
            *vis = Visibility::Inherited;
        }
        *was = false;
        return;
    }
    *was = true;
    let Ok(mut tf) = cam.single_mut() else { return };
    tf.translation = (b.pos + Vec3::Y * p.height() * 0.92) * SCALE;
    let dir = Vec3::new(fp.yaw.sin() * fp.pitch.cos(), fp.pitch.sin(), fp.yaw.cos() * fp.pitch.cos());
    tf.look_to(dir, Vec3::Y);
}
