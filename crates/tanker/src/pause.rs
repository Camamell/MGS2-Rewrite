//! Pause: P (Start on a pad) stops game time and holds back every other
//! input until it is pressed again (not during cutscenes, Codec calls or
//! the game-over screen).

use bevy::input::InputSystems;
use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct Paused(pub bool);

#[derive(Component)]
struct PauseText;

pub fn plugin(app: &mut App) {
    app.init_resource::<Paused>()
        .add_systems(Startup, setup)
        .add_systems(PreUpdate, toggle.after(InputSystems).after(crate::pad::feed).run_if(crate::codec::idle).run_if(crate::demo::idle))
        .add_systems(Update, (show, fullscreen));
}

/// F11 or Alt+Enter: fullscreen (borderless) on and off.
pub fn fullscreen(keys: Res<ButtonInput<KeyCode>>, mut windows: Query<&mut Window>) {
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    if !keys.just_pressed(KeyCode::F11) && !(alt && keys.just_pressed(KeyCode::Enter)) {
        return;
    }
    for mut w in &mut windows {
        w.mode = match w.mode {
            bevy::window::WindowMode::Windowed => bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current),
            _ => bevy::window::WindowMode::Windowed,
        };
    }
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Text::new("PAUSE"),
        TextFont { font_size: bevy::text::FontSize::Px(48.0), ..default() },
        TextColor(Color::srgb(0.85, 0.9, 0.88)),
        Node { position_type: PositionType::Absolute, top: Val::Percent(45.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
        TextLayout::justify(Justify::Center),
        Visibility::Hidden,
        PauseText,
    ));
}

fn toggle(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut paused: ResMut<Paused>,
    mut time: ResMut<Time<Virtual>>,
    player: Query<&crate::player::Player>,
    real: Res<Time<Real>>,
    mut fired: Local<bool>,
) {
    // Not over the game-over screen (Start continues there).
    let dead = player.single().is_ok_and(|p| p.dead());
    // MGS2_PAUSE_AT=seconds: press pause then (testing).
    let scripted = std::env::var("MGS2_PAUSE_AT").ok().and_then(|v| v.parse::<f32>().ok()).is_some_and(|at| {
        let due = !*fired && real.elapsed_secs() >= at;
        *fired |= due;
        due
    });
    if (keys.just_pressed(KeyCode::KeyP) || scripted) && (!dead || paused.0) {
        paused.0 = !paused.0;
        if paused.0 {
            time.pause();
        } else {
            time.unpause();
        }
    }
    if paused.0 {
        keys.reset_all();
    }
}

fn show(paused: Res<Paused>, mut q: Query<&mut Visibility, With<PauseText>>) {
    let want = if paused.0 { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut q {
        if *v != want {
            *v = want;
        }
    }
}
