//! Thermal goggles (item 13): T puts them on or takes them off once Snake
//! has them (`MGS2_GOGGLES=1` starts with them on, for testing). The game's goggle filter
//! is not ported; here the scene goes dark and blue while the characters
//! glow warm (assumed look).

use bevy::prelude::*;

use crate::body::Body;

/// Inventory key of the goggles.
pub const GOGGLES: (i32, i32) = (0, 13);

#[derive(Component)]
struct Veil;

#[derive(Resource, Default)]
pub struct Thermal {
    pub on: bool,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Thermal>().add_systems(Update, (toggle, view).chain());
}

fn toggle(keys: Res<ButtonInput<KeyCode>>, mut inv: ResMut<crate::items::Inventory>, mut t: ResMut<Thermal>, mut started: Local<bool>) {
    let test = std::env::var_os("MGS2_GOGGLES").is_some();
    if test && !*started {
        *started = true;
        t.on = true;
        return;
    }
    let have = inv.counts.get(&GOGGLES).copied().unwrap_or(0) > 0 || test;
    if !have {
        t.on = false;
        return;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        t.on = !t.on;
        inv.message = Some((if t.on { "Thermal Goggles on" } else { "Thermal Goggles off" }.into(), 1.5));
    }
}

/// The filter: the camera's grading, and the characters' materials made to
/// glow.
fn view(
    t: Res<Thermal>,
    mut commands: Commands,
    veil: Query<Entity, With<Veil>>,
    bodies: Query<Entity, With<Body>>,
    children: Query<&Children>,
    mats: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shown: Local<bool>,
    mut clock: Local<f32>,
    time: Res<Time>,
) {
    *clock += time.delta_secs();
    // Re-glow now and then while on, for characters spawned since.
    let refresh = t.on && *clock > 1.0;
    if t.on == *shown && !refresh {
        return;
    }
    *clock = 0.0;
    *shown = t.on;
    // A dark blue veil over the scene (the camera's grading has no effect
    // with this pipeline's tonemapping).
    for e in &veil {
        commands.entity(e).despawn();
    }
    if t.on {
        commands.spawn((
            Veil,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.04, 0.2, 0.82)),
            GlobalZIndex(-1),
        ));
    }
    // The characters' materials are unlit: their colour is scaled instead
    // (bright enough to stand out of the darkened grading).
    let glow: Color = if t.on { LinearRgba::rgb(9.0, 4.0, 1.2).into() } else { Color::WHITE };
    let mut done = std::collections::HashSet::new();
    for b in &bodies {
        for c in children.iter_descendants(b) {
            let Ok(m) = mats.get(c) else { continue };
            if !done.insert(m.0.id()) {
                continue;
            }
            // (Textured parts only: not the shadows.)
            if let Some(mut mat) = materials.get_mut(&m.0)
                && mat.base_color_texture.is_some()
                && mat.base_color != glow
            {
                mat.base_color = glow;
            }
        }
    }
}
