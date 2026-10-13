//! `--title`: a start menu (New game with a difficulty, Continue from the
//! autosave, Quit). Stages are prepared before the game's window opens, so
//! the choice relaunches the program with the matching arguments.
//! (The game's own title screen, `n_title`, is not reproduced.)

use bevy::prelude::*;

const GREEN: Color = Color::srgb(0.25, 0.85, 0.45);
const DIM: Color = Color::srgb(0.35, 0.5, 0.4);

const STAGES: [&str; 15] = ["w00a", "w00b", "w00c", "w01a", "w01b", "w01c", "w01d", "w01e", "w01f", "w02a", "w03a", "w03b", "w04a", "w04b", "w04c"];

const DIFFICULTIES: [(i16, &str); 5] = [(10, "VERY EASY"), (20, "EASY"), (30, "NORMAL"), (40, "HARD"), (50, "EXTREME")];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    NewGame,
    Continue,
    Stage,
    Quit,
}

#[derive(Resource)]
struct Menu {
    items: Vec<Item>,
    at: usize,
    difficulty: usize,
    stage: usize,
    /// Extra arguments passed through to the game (`--iso ...`).
    pass: Vec<String>,
    chosen: Option<Vec<String>>,
}

#[derive(Component)]
struct Line(usize);

/// Runs the menu; returns the arguments to start the game with (None: quit).
pub fn run(pass: Vec<String>, difficulty: i16) -> Option<Vec<String>> {
    let mut items = vec![Item::NewGame];
    if crate::save::read().is_some() {
        items.insert(0, Item::Continue);
    }
    items.push(Item::Stage);
    items.push(Item::Quit);
    let difficulty = DIFFICULTIES.iter().position(|d| d.0 == difficulty).unwrap_or(2);
    let chosen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let out = chosen.clone();
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "MGS2 Tanker".into(), ..default() }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.0, 0.0, 0.0)))
        .insert_resource(Menu { items, at: 0, difficulty, stage: 0, pass, chosen: None })
        .add_systems(Startup, setup)
        .add_systems(Update, (input, draw, shot).chain())
        .add_systems(Last, move |menu: Res<Menu>, mut exit: MessageWriter<AppExit>| {
            if let Some(args) = &menu.chosen {
                *out.lock().unwrap() = Some(args.clone());
                exit.write(AppExit::Success);
            }
        })
        .run();
    chosen.lock().unwrap().take()
}

fn setup(mut commands: Commands, menu: Res<Menu>) {
    commands.spawn(Camera2d);
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(14.0),
            ..default()
        })
        .with_children(|p| {
            p.spawn((Text::new("METAL GEAR SOLID 2"), TextFont { font_size: bevy::text::FontSize::Px(54.0), ..default() }, TextColor(Color::srgb(0.85, 0.9, 0.88))));
            p.spawn((Text::new("SONS OF LIBERTY  -  TANKER"), TextFont { font_size: bevy::text::FontSize::Px(22.0), ..default() }, TextColor(DIM)));
            p.spawn(Node { height: Val::Px(40.0), ..default() });
            for i in 0..menu.items.len() {
                p.spawn((Text::new(""), TextFont { font_size: bevy::text::FontSize::Px(30.0), ..default() }, TextColor(DIM), Line(i)));
            }
            p.spawn(Node { height: Val::Px(40.0), ..default() });
            p.spawn((
                Text::new("W/S choose   A/D difficulty   Q/E stage   Enter start   Esc quit"),
                TextFont { font_size: bevy::text::FontSize::Px(16.0), ..default() },
                TextColor(DIM),
            ));
        });
}

fn input(keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<Menu>, mut exit: MessageWriter<AppExit>) {
    let n = menu.items.len();
    if keys.any_just_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        menu.at = (menu.at + n - 1) % n;
    }
    if keys.any_just_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        menu.at = (menu.at + 1) % n;
    }
    let left = keys.any_just_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]);
    let right = keys.any_just_pressed([KeyCode::KeyD, KeyCode::ArrowRight]);
    match menu.items[menu.at] {
        Item::NewGame | Item::Stage if left => menu.difficulty = menu.difficulty.saturating_sub(1),
        Item::NewGame | Item::Stage if right => menu.difficulty = (menu.difficulty + 1).min(DIFFICULTIES.len() - 1),
        _ => {}
    }
    if menu.items[menu.at] == Item::Stage {
        if keys.any_just_pressed([KeyCode::KeyQ, KeyCode::PageUp]) {
            menu.stage = (menu.stage + STAGES.len() - 1) % STAGES.len();
        }
        if keys.any_just_pressed([KeyCode::KeyE, KeyCode::PageDown]) {
            menu.stage = (menu.stage + 1) % STAGES.len();
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
        return;
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) {
        let mut args = menu.pass.clone();
        match menu.items[menu.at] {
            Item::NewGame => args.extend(["--difficulty".to_string(), DIFFICULTIES[menu.difficulty].0.to_string()]),
            Item::Continue => args.push("--continue".into()),
            Item::Stage => args.extend([STAGES[menu.stage].to_string(), "--difficulty".to_string(), DIFFICULTIES[menu.difficulty].0.to_string()]),
            Item::Quit => {
                exit.write(AppExit::Success);
                return;
            }
        }
        menu.chosen = Some(args);
    }
}

/// `MGS2_TITLE_SHOT=<png>`: save a screenshot of the menu, then quit
/// (testing).
fn shot(mut commands: Commands, time: Res<Time>, mut done: Local<bool>, mut exit: MessageWriter<AppExit>) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let Some(path) = std::env::var_os("MGS2_TITLE_SHOT") else { return };
    let t = time.elapsed_secs();
    if t > 1.5 && !*done {
        *done = true;
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(std::path::PathBuf::from(path)));
    }
    if t > 2.5 {
        exit.write(AppExit::Success);
    }
}

fn draw(menu: Res<Menu>, mut lines: Query<(&Line, &mut Text, &mut TextColor)>) {
    for (l, mut t, mut c) in &mut lines {
        let label = match menu.items[l.0] {
            Item::NewGame => format!("NEW GAME   < {} >", DIFFICULTIES[menu.difficulty].1),
            Item::Continue => "CONTINUE".to_string(),
            Item::Stage => format!("STAGE SELECT   [ {} ]   < {} >", STAGES[menu.stage].to_uppercase(), DIFFICULTIES[menu.difficulty].1),
            Item::Quit => "EXIT".to_string(),
        };
        let selected = l.0 == menu.at;
        let s = if selected { format!("> {label} <") } else { label };
        if **t != s {
            **t = s;
        }
        let colour = if selected { GREEN } else { DIM };
        if c.0 != colour {
            c.0 = colour;
        }
    }
}
