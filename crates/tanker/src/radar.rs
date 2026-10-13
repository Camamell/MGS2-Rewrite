//! The Soliton radar (`radar.c`: every stage script creates it with
//! `chara #1af92a [-d angle]`, `cmd_1af92a` in the stage residents).
//!
//! The game builds it from its own textures (set 0x715d80) and turns the
//! map by `-d` (4096ths of a turn, `FUN_0031abd8`); its scale is 0.006
//! screen units per mm (actor +0x6e0). This port draws the stage's
//! collision walls around Snake's height, Snake, and the guards with their
//! vision cones, top-down in a small panel; in Alert and Evasion the map
//! gives way to the phase's name, as in the game.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::body::Body;
use crate::guard::{Guard, Phase, PhaseKind};
use crate::player::Player;
use crate::{Game, SCALE};

pub const RADAR_ACTOR: u32 = 0x1AF92A;

/// Render layer of the radar's camera and lines.
const LAYER: usize = 6;
/// Panel size in pixels.
const W: u32 = 220;
const H: u32 = 168;
/// Width of the area shown (mm): the game's 0.006 per mm over a panel of
/// about 140 of its 640 screen units.
const SPAN: f32 = 23_000.0;
/// Where the radar's lines are drawn, far from the level (m).
const ORIGIN: Vec3 = Vec3::new(0.0, -5000.0, 0.0);

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct RadarGizmos;

#[derive(Component)]
struct RadarPanel;

#[derive(Component)]
struct RadarCamera;

#[derive(Component)]
struct RadarLabel;

/// The radar's turn (radians) from the actor's `-d`.
pub fn angle(s: &crate::gcl::Spawn) -> f32 {
    let mut d = s.opt_int(b'd', 0).unwrap_or(0) & 0xFFF;
    if d > 0x800 {
        d -= 0x1000;
    }
    -(d as f32) * std::f32::consts::TAU / 4096.0
}

pub fn plugin(app: &mut App) {
    app.init_gizmo_group::<RadarGizmos>().add_systems(Startup, setup).add_systems(Update, draw);
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<RadarGizmos>();
    config.render_layers = RenderLayers::layer(LAYER);
    config.line.width = 1.5;
    let image = images.add(Image::new_target_texture(W, H, TextureFormat::Rgba8UnormSrgb, None));
    commands.spawn((
        RadarCamera,
        Camera3d::default(),
        Camera { order: -10, clear_color: ClearColorConfig::Custom(Color::srgba(0.0, 0.08, 0.05, 0.8)), is_active: false, ..default() },
        RenderTarget::from(image.clone()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed { width: SPAN * SCALE, height: SPAN * SCALE * H as f32 / W as f32 },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(ORIGIN + Vec3::Y * 50.0).looking_at(ORIGIN, Vec3::NEG_Z),
        RenderLayers::layer(LAYER),
    ));
    commands
        .spawn((
            RadarPanel,
            ImageNode::new(image),
            Node {
                position_type: PositionType::Absolute,
                top: px(10),
                right: px(10),
                width: px(W as f32),
                height: px(H as f32),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(Color::srgb(0.3, 0.8, 0.5)),
            Visibility::Hidden,
        ))
        .with_child((
            RadarLabel,
            Text::new(""),
            TextFont { font_size: bevy::text::FontSize::Px(22.0), ..default() },
            TextColor(Color::srgb(1.0, 0.25, 0.2)),
        ));
}

#[allow(clippy::too_many_arguments)]
fn draw(
    game: Res<Game>,
    phase: Option<Res<Phase>>,
    cutscene: Res<crate::demo::Cutscene>,
    codec: Res<crate::codec::CodecState>,
    jam: Res<crate::grenade::Jam>,
    snake: Query<&Body, With<Player>>,
    guards: Query<(&Guard, &Body), Without<Player>>,
    bosses: Query<&Body, (With<crate::olga::Olga>, Without<Player>)>,
    mut panel: Query<&mut Visibility, With<RadarPanel>>,
    mut label: Query<&mut Text, With<RadarLabel>>,
    mut cams: Query<&mut Camera, With<RadarCamera>>,
    mut gizmos: Gizmos<RadarGizmos>,
) {
    let snake = snake.single().ok();
    let shown = game.radar.is_some()
        && snake.is_some()
        && !cutscene.active()
        && codec.call.is_none()
        && game.disabled & crate::gcl::flags::RADAR == 0;
    if let Ok(mut v) = panel.single_mut() {
        let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if let Ok(mut c) = cams.single_mut()
        && c.is_active != shown
    {
        c.is_active = shown;
    }
    let (Some(turn), Some(snake), true) = (game.radar, snake, shown) else { return };
    let kind = phase.map_or(PhaseKind::Infiltration, |p| p.kind);
    let chaff = jam.0 > 0.0;
    let jammed = chaff || matches!(kind, PhaseKind::Alert | PhaseKind::Evasion);
    if let Ok(mut t) = label.single_mut() {
        let want = match kind {
            _ if chaff => "JAMMING",
            PhaseKind::Alert => "ALERT",
            PhaseKind::Evasion => "EVASION",
            _ => "",
        };
        if t.0 != want {
            t.0 = want.to_string();
        }
    }
    if jammed {
        return;
    }
    let rot = Quat::from_rotation_y(turn);
    let at = |p: Vec3| ORIGIN + rot * ((p - snake.pos) * SCALE * Vec3::new(1.0, 0.0, 1.0));
    // Walls standing across Snake's waist height.
    let waist = snake.pos.y + 900.0;
    if let Some(h) = &game.data.collision {
        for w in h.areas.iter().flat_map(|a| &a.groups).flat_map(|g| &g.walls) {
            for pair in w.points.windows(2) {
                let ((a, ha), (b, hb)) = (pair[0], pair[1]);
                let (a, b) = (Vec3::from(a), Vec3::from(b));
                let (lo, hi) = (a.y.min(b.y), (a.y + ha).max(b.y + hb));
                if waist < lo || waist > hi {
                    continue;
                }
                let near = |p: Vec3| (p - snake.pos).xz().length() < SPAN;
                if near(a) || near(b) {
                    gizmos.line(at(a), at(b), Color::srgb(0.35, 1.0, 0.55));
                }
            }
        }
    }
    for (g, b) in &guards {
        if (b.pos.y - snake.pos.y).abs() > 2500.0 {
            continue;
        }
        let c = at(b.pos);
        let half = g.vision.half_angle as f32 * std::f32::consts::TAU / 4096.0;
        let range = g.vision.distance * SCALE;
        let dir = |yaw: f32| rot * Vec3::new(yaw.sin(), 0.0, yaw.cos());
        let cone = Color::srgba(0.4, 0.8, 1.0, 0.6);
        let edges: Vec<Vec3> = (0..=8).map(|i| c + dir(b.yaw - half + 2.0 * half * i as f32 / 8.0) * range).collect();
        gizmos.line(c, edges[0], cone);
        gizmos.line(c, edges[8], cone);
        gizmos.linestrip(edges, cone);
        gizmos.circle(Isometry3d::new(c, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), 0.25, Color::srgb(1.0, 0.2, 0.15));
    }
    for b in &bosses {
        gizmos.circle(Isometry3d::new(at(b.pos), Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), 0.3, Color::srgb(1.0, 0.2, 0.15));
    }
    // Snake: a white arrow at the centre.
    let f = rot * Vec3::new(snake.yaw.sin(), 0.0, snake.yaw.cos());
    let side = Vec3::new(-f.z, 0.0, f.x);
    let tip = ORIGIN + f * 0.6;
    let white = Color::WHITE;
    gizmos.line(ORIGIN - f * 0.4 + side * 0.35, tip, white);
    gizmos.line(ORIGIN - f * 0.4 - side * 0.35, tip, white);
    gizmos.line(ORIGIN - f * 0.4 + side * 0.35, ORIGIN - f * 0.4 - side * 0.35, white);
}
