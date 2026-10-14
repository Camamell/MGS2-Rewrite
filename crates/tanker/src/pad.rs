//! Gamepad support: the pad's buttons and sticks are fed to the game as the
//! keyboard keys they stand for, so every control works on a pad without
//! touching the systems that read the keys.
//!
//! | Pad (PS / Xbox) | Key | Action |
//! |---|---|---|
//! | left stick | W A S D (+ Shift when tilted gently) | run / walk |
//! | right stick left / right | Q / E | turn the camera |
//! | Cross / A | C | crouch, roll; camera in the holds |
//! | Circle / B | Space | action |
//! | Square / X | F | aim and fire (hold, release) |
//! | Triangle / Y | V | punch |
//! | R1 / RB | X | first-person view |
//! | L1 / LB | Z | prone |
//! | R2 / RT | G | grenade |
//! | L2 / LT | R | ration |
//! | D-pad left / right | 1 / 2 | M9 / USP |
//! | D-pad up | B | cardboard box |
//! | D-pad down | K | Codec |
//! | Start | P, Enter, Esc | pause; continue / skip a cutscene, hang up |
//! | Select | L | Codec 140.96 (save) |
//!
//! The pad rumbles when Snake is hit (the game's `vibrate.c` shakes it too;
//! strength and length here are assumed).

use bevy::input::InputSystems;
use bevy::prelude::*;

/// Set for automated runs (`--screenshot`, or `MGS2_NO_PAD=1`): the pad is
/// ignored, so a pad in use elsewhere cannot steer a test.
pub static DISABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The left stick (y forward, x right), for analog steering.
#[derive(Resource, Default)]
pub struct Stick(pub Vec2);

/// Keys the pad is holding down (so physical key presses are left alone).
#[derive(Resource, Default)]
pub struct Held(Vec<KeyCode>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Held>().init_resource::<Stick>().add_systems(PreUpdate, feed.after(InputSystems));
}

/// Rumble when Snake takes a hit (the game plugin adds this; the start menu
/// has no player).
pub fn rumble(
    player: Query<&crate::player::Player>,
    pads: Query<Entity, With<Gamepad>>,
    mut last_life: Local<Option<i32>>,
    mut out: MessageWriter<bevy::input::gamepad::GamepadRumbleRequest>,
) {
    use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
    let Ok(p) = player.single() else { return };
    if last_life.is_some_and(|l| p.life < l) {
        let intensity = if p.dead() { GamepadRumbleIntensity::MAX } else { GamepadRumbleIntensity { strong_motor: 0.6, weak_motor: 0.4 } };
        for gamepad in &pads {
            out.write(GamepadRumbleRequest::Add { duration: std::time::Duration::from_millis(if p.dead() { 600 } else { 180 }), intensity, gamepad });
        }
    }
    *last_life = Some(p.life);
}

const BUTTONS: [(GamepadButton, &[KeyCode]); 14] = [
    (GamepadButton::South, &[KeyCode::KeyC]),
    (GamepadButton::East, &[KeyCode::Space]),
    (GamepadButton::West, &[KeyCode::KeyF]),
    (GamepadButton::North, &[KeyCode::KeyV]),
    (GamepadButton::RightTrigger, &[KeyCode::KeyX]),
    (GamepadButton::LeftTrigger, &[KeyCode::KeyZ]),
    (GamepadButton::RightTrigger2, &[KeyCode::KeyG]),
    (GamepadButton::LeftTrigger2, &[KeyCode::KeyR]),
    (GamepadButton::DPadLeft, &[KeyCode::Digit1]),
    (GamepadButton::DPadRight, &[KeyCode::Digit2]),
    (GamepadButton::DPadUp, &[KeyCode::KeyB]),
    (GamepadButton::DPadDown, &[KeyCode::KeyK]),
    (GamepadButton::Start, &[KeyCode::KeyP, KeyCode::Enter, KeyCode::Escape]),
    (GamepadButton::Select, &[KeyCode::KeyL]),
];

/// Stick deflection that counts as a direction, and below which a held
/// direction means walking.
const DEAD: f32 = 0.3;
const WALK: f32 = 0.7;

pub fn feed(
    pads: Query<&Gamepad>,
    time: Res<Time<Real>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut held: ResMut<Held>,
    mut stick: ResMut<Stick>,
    mut fuzz: Local<(u32, Vec<KeyCode>)>,
) {
    let mut want: Vec<KeyCode> = Vec::new();
    stick.0 = Vec2::ZERO;
    // MGS2_FUZZ=seed: random controls, changed every half second (looking
    // for crashes). Esc and P are left out so runs do not stall.
    if let Some(seed) = std::env::var("MGS2_FUZZ").ok().and_then(|v| v.parse::<u32>().ok()) {
        let tick = (time.elapsed_secs() * 2.0) as u32;
        if tick != fuzz.0 || fuzz.1.is_empty() {
            fuzz.0 = tick;
            let mut x = seed.wrapping_mul(0x9E37_79B9) ^ tick.wrapping_mul(0x85EB_CA6B) | 1;
            let mut next = || {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x
            };
            const POOL: [KeyCode; 17] = [
                KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD, KeyCode::ShiftLeft, KeyCode::KeyC, KeyCode::Space, KeyCode::KeyF,
                KeyCode::KeyV, KeyCode::KeyX, KeyCode::KeyZ, KeyCode::KeyG, KeyCode::KeyR, KeyCode::Digit1, KeyCode::Digit2, KeyCode::KeyB, KeyCode::KeyK,
            ];
            fuzz.1 = (0..1 + next() % 3).map(|_| POOL[(next() % POOL.len() as u32) as usize]).collect();
        }
        want.extend(fuzz.1.iter().copied());
    }
    let off = DISABLED.load(std::sync::atomic::Ordering::Relaxed) || std::env::var_os("MGS2_NO_PAD").is_some();
    for pad in pads.iter().filter(|_| !off) {
        // MGS2_PAD_LOG=1: what the pad reports (checking a pad's resting state).
        if std::env::var_os("MGS2_PAD_LOG").is_some() {
            let down: Vec<String> = BUTTONS.iter().filter(|(b, _)| pad.pressed(*b)).map(|(b, _)| format!("{b:?}")).collect();
            if !down.is_empty() || pad.left_stick().length() > DEAD || pad.right_stick().length() > 0.5 {
                info!("pad: {down:?} left {:?} right {:?}", pad.left_stick(), pad.right_stick());
            }
        }
        for (button, mapped) in BUTTONS {
            if pad.pressed(button) {
                want.extend_from_slice(mapped);
            }
        }
        let s = pad.left_stick();
        if s.length() > DEAD {
            stick.0 = s;
        }
        if s.y > DEAD {
            want.push(KeyCode::KeyW);
        }
        if s.y < -DEAD {
            want.push(KeyCode::KeyS);
        }
        if s.x < -DEAD {
            want.push(KeyCode::KeyA);
        }
        if s.x > DEAD {
            want.push(KeyCode::KeyD);
        }
        if s.length() > DEAD && s.length() < WALK {
            want.push(KeyCode::ShiftLeft);
        }
        let r = pad.right_stick();
        if r.x < -0.5 {
            want.push(KeyCode::KeyQ);
        }
        if r.x > 0.5 {
            want.push(KeyCode::KeyE);
        }
    }
    for k in held.0.clone() {
        if !want.contains(&k) {
            keys.release(k);
        }
    }
    for &k in &want {
        keys.press(k);
    }
    held.0 = want;
}
