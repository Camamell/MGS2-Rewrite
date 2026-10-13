//! Lightning over the open decks: `chara #89af5e [-p chance] [-t]` (w00a-c,
//! w01e; `cmd_89af5e`, act `FUN_0029aa08` in their residents).
//!
//! Each frame it strikes with chance 1 / `-p` (default 0x1000). A strike
//! (unless `-t`) draws a bolt (`d_thunder.c`) and lights five
//! copies of a 64 m sky quad (model 0x3177fc, texture 0x985073), each turned
//! at random and spread over the sky (`FUN_0029a3c0`), tinted (25, 50, 50)
//! +-5 (its other tint, (18, 30, 30), is never chosen), for 10..29 frames: they
//! flicker (each frame shown or not at random) and fade by 10 a frame over
//! the last 10. Its thunder follows 60..123 frames later (effect 0x1d5 /
//! 0x1d6), or 30..61 (0x1d8 / 0x1d9) for a near one (`FUN_0029a9c0`). The
//! game places the quads around the bolt's point in the sky, which it picks
//! on screen; here they go up ahead of the camera. The bolt: [`Bolts`].

use bevy::prelude::*;

use crate::{Game, SCALE};

pub const THUNDER_ACTOR: u32 = 0x89AF5E;
const FLASH_MODEL: u32 = 0x3177FC;

#[derive(Clone, Copy, Debug)]
pub struct ThunderSpec {
    /// One strike in this many frames on average.
    pub chance: u32,
    /// `-t`: no bolt.
    pub no_bolt: bool,
}

pub fn from_spawn(s: &crate::gcl::Spawn) -> ThunderSpec {
    // MGS2_THUNDER=n: one strike in n frames instead (testing).
    let test = std::env::var("MGS2_THUNDER").ok().and_then(|v| v.parse().ok());
    ThunderSpec { chance: test.unwrap_or(s.opt_int(b'p', 0).filter(|p| *p > 0).unwrap_or(0x1000) as u32).max(1), no_bolt: s.opt(b't').is_some() }
}

/// A lit sky quad.
#[derive(Component)]
struct Flash {
    material: Handle<StandardMaterial>,
}

#[derive(Default)]
struct State {
    /// Frames the flash still lasts (-1: none).
    left: i32,
    colour: Vec3,
    /// (frames until the thunder, effect).
    sound: Option<(i32, u32)>,
    /// 60 Hz frame accumulator.
    acc: f32,
    rng: u32,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, run);
}

fn rand(s: &mut State) -> u32 {
    // The game's generator (`DAT_0019227c`).
    s.rng = s.rng.wrapping_mul(0x5D58_8B65).wrapping_add(1);
    s.rng
}

#[allow(clippy::too_many_arguments)]
fn run(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    cutscene: Res<crate::demo::Cutscene>,
    cams: Query<&Transform, With<crate::player::FollowCam>>,
    mut flashes: Query<(Entity, &Flash, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut st: Local<State>,
    mut bolts: ResMut<Bolts>,
    projs: Query<&Projection, With<crate::player::FollowCam>>,
    windows: Query<&Window>,
) {
    let Some(spec) = game.thunder else {
        for (e, _, _) in &flashes {
            commands.entity(e).despawn();
        }
        return;
    };
    if st.rng == 0 {
        st.rng = 0x1234_5678;
        st.left = -1;
    }
    st.acc += time.delta_secs() * 60.0;
    while st.acc >= 1.0 {
        st.acc -= 1.0;
        // The thunder clap's countdown runs regardless.
        if let Some((n, id)) = st.sound.as_mut() {
            *n -= 1;
            if *n <= 0 {
                crate::sfx::play(*id, None);
                st.sound = None;
            }
        }
        if cutscene.active() {
            continue;
        }
        if st.left < 0 {
            let r = rand(&mut st);
            if (r % spec.chance) != 0 {
                continue;
            }
            // A strike.
            st.left = ((rand(&mut st) >> 8) % 20 + 10) as i32;
            let jitter = (rand(&mut st) as f32 * 2.328_306_4e-10 * 10.0 - 5.0).trunc();
            st.colour = Vec3::new(jitter + 25.0, jitter + 50.0, jitter + 50.0);
            let near = rand(&mut st) >> 8 & 1 != 0;
            let (frames, id) = if near {
                ((rand(&mut st) & 0x1F) as i32 + 0x1E, if rand(&mut st) >> 8 & 1 == 0 { 0x1D9 } else { 0x1D8 })
            } else {
                ((rand(&mut st) & 0x3F) as i32 + 0x3C, if rand(&mut st) >> 8 & 1 == 0 { 0x1D6 } else { 0x1D5 })
            };
            st.sound = Some((frames, id));
            if !spec.no_bolt
                && let Ok(cam) = cams.single()
            {
                // FUN_0029aa08: x -1..1, y -2..0 on screen.
                let x = rand(&mut st) as f32 * 4.656_613e-10 - 1.0;
                let y = -(rand(&mut st) as f32 * 2.328_306_4e-10) * 2.0;
                let fov = match projs.single() {
                    Ok(Projection::Perspective(p)) => p.fov,
                    _ => 0.8,
                };
                let aspect = windows.single().map_or(16.0 / 9.0, |w| w.width() / w.height().max(1.0));
                bolts.strike(cam, fov, aspect, [x, y]);
            }
            spawn_flashes(&mut commands, &game, &cams, &mut st, &mut meshes, &mut images, &mut materials);
        } else {
            st.left -= 1;
            if st.left < 10 {
                st.colour = (st.colour - Vec3::splat(10.0)).max(Vec3::ZERO);
            }
        }
    }
    let on = st.left >= 0;
    for (e, f, mut vis) in &mut flashes {
        if !on {
            commands.entity(e).despawn();
            continue;
        }
        let shown = rand(&mut st) >> 8 & 1 != 0;
        *vis = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if let Some(mut m) = materials.get_mut(&f.material) {
            let c = st.colour / 128.0;
            m.base_color = Color::linear_rgb(c.x, c.y, c.z);
        }
    }
    if st.left == 0 {
        st.left = -1;
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_flashes(
    commands: &mut Commands,
    game: &Game,
    cams: &Query<&Transform, With<crate::player::FollowCam>>,
    st: &mut State,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let data = &game.data;
    let Some((_, model)) = data.models.iter().find(|(h, _)| *h & 0x00FF_FFFF == FLASH_MODEL) else { return };
    let Ok(cam) = cams.single() else { return };
    let ahead = (cam.forward().as_vec3() * Vec3::new(1.0, 0.0, 1.0)).normalize_or(Vec3::Z);
    // Up in the sky ahead of the camera (mm).
    let base = cam.translation / SCALE + ahead * 120_000.0 + Vec3::Y * 50_000.0;
    let parts = mgs2_bevy::scene::model_meshes(model);
    for i in 0..5 {
        let mut at = base;
        if i > 0 {
            let dx = rand(st) as f32 * 4.656_613e-10 - 1.0;
            let dz = rand(st) as f32 * 4.656_613e-10 - 1.0;
            at += Vec3::new(dx, 0.0, dz) * 50_000.0;
        }
        let turn = (rand(st) & 0xFFF) as f32 / 4096.0 * std::f32::consts::TAU;
        let place = Transform::from_translation(at * SCALE).with_rotation(Quat::from_rotation_y(turn));
        let Some((tex, mesh, _)) = parts.first() else { return };
        let material = materials.add(StandardMaterial {
            base_color_texture: data.textures.get(tex).map(|t| images.add(mgs2_bevy::scene::image(t))),
            base_color: Color::BLACK,
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            fog_enabled: false,
            ..default()
        });
        commands.spawn((crate::LevelEntity, Mesh3d(meshes.add(mesh.clone())), MeshMaterial3d(material.clone()), Flash { material }, place, Visibility::Hidden));
    }
}

/// A lightning bolt (`d_thunder.c`: `FUN_0029d1e0` from a point on screen,
/// `FUN_0029cee8` puts both ends 1 km out, `FUN_0029ea00` / `FUN_0029e7a8`
/// build it, `FUN_0029daa0` / `FUN_0029d548` lay 65 points by midpoint
/// displacement (a quarter of the span, random direction), `FUN_0029e118`
/// fades it). Three crossed ribbons L x 0.007 wide in the game, coloured
/// (0x30, 0xc0, 0x60) with alpha up to 0x40: rising over the first 16
/// points, then down to the tip (a branch: from full at its root); each
/// bolt branches once more (half as long, one level fewer) and lives 23
/// frames (a branch one less), fading over its last 12. Here: one ribbon
/// facing the camera, twice as wide, 500 m out.
struct Bolt {
    pts: Vec<Vec3>,
    width: f32,
    main: bool,
    life: f32,
}

#[derive(Resource, Default)]
pub struct Bolts {
    list: Vec<Bolt>,
    rng: u32,
    /// The cutscene frame last checked for bolts.
    last: Option<(usize, f32)>,
}

#[derive(Component)]
struct BoltMesh;

impl Bolts {
    fn rand(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(0x5D58_8B65).wrapping_add(1);
        self.rng as f32 * 2.328_306_4e-10
    }

    fn signed(&mut self) -> f32 {
        self.rand() * 2.0 - 1.0
    }

    /// 65 points from `a` to `b`, each midpoint pushed off by up to a
    /// quarter of its span.
    fn path(&mut self, a: Vec3, b: Vec3) -> Vec<Vec3> {
        let mut pts = vec![Vec3::ZERO; 65];
        pts[0] = a;
        pts[64] = b;
        let mut stack = vec![(0usize, 64usize)];
        while let Some((lo, hi)) = stack.pop() {
            if hi - lo < 2 {
                continue;
            }
            let mid = (lo + hi) / 2;
            let span = pts[hi] - pts[lo];
            let axis = span.normalize_or(Vec3::Y);
            let side = axis.any_orthonormal_vector();
            let turn = Quat::from_axis_angle(axis, self.rand() * std::f32::consts::TAU);
            pts[mid] = (pts[lo] + pts[hi]) / 2.0 + turn * side * span.length() * 0.25 * self.signed();
            stack.push((lo, mid));
            stack.push((mid, hi));
        }
        pts
    }

    /// A bolt and its branches.
    fn add(&mut self, a: Vec3, b: Vec3, levels: u32, life: f32, main: bool) {
        let pts = self.path(a, b);
        let len = a.distance(b).max(10.0);
        if levels > 0 {
            let from = pts[(self.rng as usize & 0x3E).min(62)];
            let dir = (b - a).normalize_or(Vec3::NEG_Y);
            let spread = 0x200 as f32 / 4096.0 * std::f32::consts::TAU;
            let rot = Quat::from_euler(EulerRot::YXZ, self.signed() * spread, self.signed() * spread, self.signed() * spread);
            let to = from + rot * dir * len * 0.5;
            self.add(from, to, levels - 1, life - 1.0, false);
        }
        // At least ~1.2 m: a few pixels 500 m out (the game's glow texture
        // widens it on screen).
        self.list.push(Bolt { pts, width: (len * 0.007 * 2.0).max(1200.0), main, life });
    }

    /// A bolt from (x, y) on screen (the game's units: x -1..1, y -2..0
    /// upwards from the centre; the far end 0.2..0.4 lower).
    pub fn strike(&mut self, cam: &Transform, fov: f32, aspect: f32, screen: [f32; 2]) {
        if self.rng == 0 {
            self.rng = 0x0BAD_5EED;
        }
        let (tan_v, tan_h) = ((fov / 2.0).tan(), (fov / 2.0).tan() * aspect);
        let (px, py) = (screen[0], screen[1]);
        let ex = px + self.signed() * 0.1;
        let ey = py + self.rand() * 0.2 + 0.2;
        let depth = 500_000.0;
        let eye = cam.translation / SCALE;
        let (fwd, right, up) = (cam.forward().as_vec3(), cam.right().as_vec3(), cam.up().as_vec3());
        let at = |x: f32, y: f32| eye + (fwd + right * x * tan_h * 0.5 - up * y * tan_v * 0.5) * depth;
        let a = at(px, py);
        let mut b = at(ex, ey);
        b.y = b.y.max(0.0);
        self.add(a, b, 2, 23.0, true);
    }
}

pub fn bolt_plugin(app: &mut App) {
    app.init_resource::<Bolts>().add_systems(Update, (cutscene_bolts, draw_bolts).chain());
}

/// The demos' own bolts.
fn cutscene_bolts(
    cutscene: Res<crate::demo::Cutscene>,
    cams: Query<(&Transform, &Projection), With<crate::player::FollowCam>>,
    windows: Query<&Window>,
    mut bolts: ResMut<Bolts>,
) {
    let pos = cutscene.active().then(|| cutscene.position()).flatten();
    let last = std::mem::replace(&mut bolts.last, pos);
    let (Some((i, f)), Ok((cam, proj))) = (pos, cams.single()) else { return };
    let after = match last {
        Some((j, g)) if j == i && g <= f => g,
        _ => f,
    };
    let fov = match proj {
        Projection::Perspective(p) => p.fov,
        _ => 0.8,
    };
    let aspect = windows.single().map_or(16.0 / 9.0, |w| w.width() / w.height().max(1.0));
    for s in cutscene.thunders(after, f) {
        bolts.strike(cam, fov, aspect, s);
    }
}

fn draw_bolts(
    mut commands: Commands,
    time: Res<Time>,
    mut bolts: ResMut<Bolts>,
    cams: Query<&Transform, With<crate::player::FollowCam>>,
    mut mesh_q: Query<(&Mesh3d, &mut Visibility), With<BoltMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let frames = time.delta_secs() * 60.0;
    for b in &mut bolts.list {
        b.life -= frames;
    }
    bolts.list.retain(|b| b.life > 0.0);
    // Nothing to draw and already hidden: leave the mesh alone (no upload).
    if bolts.list.is_empty()
        && mesh_q.iter().all(|(_, v)| *v == Visibility::Hidden)
        && !mesh_q.is_empty()
    {
        return;
    }
    let Ok(cam) = cams.single() else { return };
    let eye = cam.translation / SCALE;
    let (mut pos, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for b in &bolts.list {
        let fade = (b.life / 12.0).min(1.0) * 0.5;
        let n = b.pts.len();
        for (k, p) in b.pts.iter().enumerate() {
            let along = if k + 1 < n { b.pts[k + 1] - *p } else { *p - b.pts[k - 1] };
            let side = along.cross(eye - *p).normalize_or(Vec3::X) * b.width * 0.5;
            let ramp = if b.main {
                if k < 16 { k as f32 / 16.0 } else { (64 - k.min(64)) as f32 / 48.0 }
            } else {
                (64 - k.min(64)) as f32 / 64.0
            };
            let a = ramp * fade;
            let c = [0.75 * a, 3.0 * a, 1.5 * a, 1.0];
            let base = pos.len() as u32;
            pos.push(((*p - side) * SCALE).to_array());
            pos.push(((*p + side) * SCALE).to_array());
            col.push(c);
            col.push(c);
            if k + 1 < n {
                idx.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
            }
        }
    }
    let empty = pos.is_empty();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    let n = pos.len().max(3);
    if empty {
        pos = vec![[0.0; 3]; 3];
        col = vec![[0.0; 4]; 3];
        idx = vec![0, 1, 2];
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
    match mesh_q.single_mut() {
        Ok((m3, mut vis)) => {
            if let Some(mut m) = meshes.get_mut(&m3.0) {
                *m = mesh;
            }
            *vis = if empty { Visibility::Hidden } else { Visibility::Inherited };
        }
        Err(_) => {
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                fog_enabled: false,
                ..default()
            });
            commands.spawn((
                BoltMesh,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                Transform::default(),
                Visibility::Hidden,
                bevy::camera::visibility::NoFrustumCulling,
            ));
        }
    }
}