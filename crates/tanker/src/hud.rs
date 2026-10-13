//! The in-game HUD laid out the way MGS2 does it: LIFE gauge top left (with
//! the boss's under it), the weapon box bottom right and the item box bottom
//! left, prompts at the bottom, the holds' timer top centre and GAME OVER in
//! the middle. Drawn with plain UI nodes (the game's own HUD textures are not
//! used). F4 shows the debug text (stage, phase, position, controls).

use bevy::prelude::*;

use crate::body::Body;
use crate::player::Player;
use crate::Game;

const GREEN: Color = Color::srgb(0.25, 0.85, 0.45);
const FRAME: Color = Color::srgba(0.7, 0.9, 0.75, 0.8);
const PANEL: Color = Color::srgba(0.0, 0.08, 0.04, 0.55);

/// HUD pieces hidden during cutscenes.
#[derive(Component)]
struct HudRoot;
#[derive(Component)]
struct LifeFill;
#[derive(Component)]
struct LifeText;
#[derive(Component)]
struct BossRow;
#[derive(Component)]
struct BossFill;
#[derive(Component)]
struct GripRow;
#[derive(Component)]
struct GripFill;
#[derive(Component)]
struct WeaponText;
#[derive(Component)]
struct ItemBox;
#[derive(Component)]
struct ItemText;
#[derive(Component)]
struct Prompt;
#[derive(Component)]
struct Centre;
#[derive(Component)]
struct TimerText;

/// Whether the debug text is shown (F4).
#[derive(Resource, Default)]
pub struct DebugHud(pub bool);

pub fn plugin(app: &mut App) {
    app.init_resource::<DebugHud>().add_systems(Startup, setup).add_systems(Update, (toggle, update));
}

fn font(size: f32) -> TextFont {
    TextFont { font_size: bevy::text::FontSize::Px(size), ..default() }
}

/// A labelled gauge: returns the row entity; the fill gets `fill`.
fn gauge(commands: &mut Commands, label: &str, colour: Color, fill: impl Bundle, row: impl Bundle) -> Entity {
    let bar = commands
        .spawn((Node { width: px(220), height: px(10), border: UiRect::all(px(1)), ..default() }, BorderColor::all(FRAME), BackgroundColor(PANEL)))
        .with_child((fill, Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(colour)))
        .id();
    let text = commands.spawn((Text::new(label), font(13.0), TextColor(FRAME), Node { width: px(46), ..default() })).id();
    commands
        .spawn((row, Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(6), ..default() }))
        .add_children(&[text, bar])
        .id()
}

fn setup(mut commands: Commands) {
    // Top left: LIFE, then the boss and the grip gauge.
    let life = gauge(&mut commands, "LIFE", GREEN, LifeFill, ());
    let life_text = commands.spawn((LifeText, Text::new(""), font(12.0), TextColor(FRAME))).id();
    let boss = gauge(&mut commands, "OLGA", Color::srgb(0.85, 0.3, 0.25), BossFill, (BossRow, Visibility::Hidden));
    let grip = gauge(&mut commands, "GRIP", Color::srgb(0.9, 0.8, 0.3), GripFill, (GripRow, Visibility::Hidden));
    commands
        .spawn((HudRoot, Node { position_type: PositionType::Absolute, top: px(14), left: px(16), flex_direction: FlexDirection::Column, row_gap: px(4), ..default() }))
        .add_children(&[life, life_text, boss, grip]);

    let corner = |left: bool| Node {
        position_type: PositionType::Absolute,
        left: if left { px(16) } else { Val::Auto },
        right: if left { Val::Auto } else { px(16) },
        bottom: px(16),
        width: px(130),
        padding: UiRect::all(px(6)),
        border: UiRect::all(px(1)),
        justify_content: JustifyContent::Center,
        ..default()
    };
    // Bottom right: weapon. Bottom left: item.
    commands
        .spawn((HudRoot, corner(false), BorderColor::all(FRAME), BackgroundColor(PANEL)))
        .with_child((WeaponText, Text::new(""), font(18.0), TextColor(Color::WHITE), TextLayout::justify(Justify::Center)));
    commands
        .spawn((ItemBox, corner(true), BorderColor::all(FRAME), BackgroundColor(PANEL), Visibility::Hidden))
        .with_child((ItemText, Text::new(""), font(18.0), TextColor(Color::WHITE), TextLayout::justify(Justify::Center)));
    // Bottom centre: prompts and pickups.
    commands.spawn((Prompt, Text::new(""), font(16.0), TextColor(Color::WHITE), TextLayout::justify(Justify::Center), Node { position_type: PositionType::Absolute, bottom: px(24), left: px(180), right: px(180), justify_content: JustifyContent::Center, ..default() }));
    // Top centre: the holds' timer.
    commands.spawn((TimerText, Text::new(""), font(22.0), TextColor(Color::srgb(1.0, 0.85, 0.3)), TextLayout::justify(Justify::Center), Node { position_type: PositionType::Absolute, top: px(14), left: px(300), right: px(300), justify_content: JustifyContent::Center, ..default() }));
    // Centre: GAME OVER.
    commands.spawn((Centre, Text::new(""), font(40.0), TextColor(Color::srgb(1.0, 0.25, 0.2)), TextLayout::justify(Justify::Center), Node { position_type: PositionType::Absolute, top: Val::Percent(40.0), left: px(0), right: px(0), justify_content: JustifyContent::Center, ..default() }));
}

fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    cutscene: Res<crate::demo::Cutscene>,
    mut debug: ResMut<DebugHud>,
    mut text: Query<&mut Visibility, With<crate::Hud>>,
    mut roots: Query<&mut Visibility, (With<HudRoot>, Without<crate::Hud>)>,
) {
    let shown = if cutscene.active() { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut roots {
        if *v != shown {
            *v = shown;
        }
    }
    if keys.just_pressed(KeyCode::F4) {
        debug.0 = !debug.0;
    }
    for mut v in &mut text {
        let want = if debug.0 && !cutscene.active() { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

fn set_text(q: &mut Query<&mut Text>, e: Entity, s: String) {
    if let Ok(mut t) = q.get_mut(e)
        && t.0 != s
    {
        t.0 = s;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update(
    game: Res<Game>,
    inv: Res<crate::items::Inventory>,
    cutscene: Res<crate::demo::Cutscene>,
    player: Query<(&Body, &Player)>,
    olgas: Query<&crate::olga::Olga>,
    mut nodes: ParamSet<(
        Query<&mut Node, With<LifeFill>>,
        Query<&mut Node, With<BossFill>>,
        Query<&mut Node, With<GripFill>>,
    )>,
    mut vis: ParamSet<(Query<&mut Visibility, With<BossRow>>, Query<&mut Visibility, With<GripRow>>, Query<&mut Visibility, With<ItemBox>>)>,
    labels: Query<(Entity, Has<LifeText>, Has<WeaponText>, Has<ItemText>, Has<Prompt>, Has<Centre>, Has<TimerText>)>,
    mut texts: Query<&mut Text>,
    mut fill_colour: Query<&mut BackgroundColor, With<LifeFill>>,
) {
    let Ok((b, p)) = player.single() else { return };
    let hidden = cutscene.active();
    let pct = |n: i32, max: i32| Val::Percent((n.max(0) as f32 * 100.0 / max.max(1) as f32).min(100.0));
    if let Ok(mut n) = nodes.p0().single_mut() {
        n.width = pct(p.life, p.max_life);
    }
    if let Ok(mut c) = fill_colour.single_mut() {
        c.0 = if p.life * 4 <= p.max_life { Color::srgb(0.9, 0.3, 0.2) } else { GREEN };
    }
    let boss = olgas.iter().next();
    if let Ok(mut v) = vis.p0().single_mut() {
        *v = if boss.is_some() && !hidden { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let (Some(o), Ok(mut n)) = (boss, nodes.p1().single_mut()) {
        n.width = pct(o.stamina, crate::olga::STAMINA);
    }
    if let Ok(mut v) = vis.p1().single_mut() {
        *v = if p.hang.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let (Some(h), Ok(mut n)) = (p.hang, nodes.p2().single_mut()) {
        n.width = pct((h.grip * 100.0) as i32, 100);
    }
    let usp = inv.usp && inv.weapon == 1;
    let ammo = inv.counts.get(&if usp { crate::items::USP_AMMO } else { crate::items::M9_AMMO }).copied().unwrap_or(0);
    let item = if p.boxed {
        Some("BOX".to_string())
    } else if inv.ration_equipped {
        Some(format!("RATION\n{}", inv.counts.get(&(0, 1)).copied().unwrap_or(0)))
    } else {
        None
    };
    if let Ok(mut v) = vis.p2().single_mut() {
        *v = if item.is_some() && !hidden { Visibility::Inherited } else { Visibility::Hidden };
    }
    let mut prompt = if crate::door_in_reach(&game, b.pos).is_some() { "[Space] open door".to_string() } else { String::new() };
    if let Some((m, _)) = &inv.message {
        if !prompt.is_empty() {
            prompt.push('\n');
        }
        prompt += &format!("Got: {m}");
    }
    for (e, life, weapon, it, pr, centre, timer) in &labels {
        let s = if hidden {
            String::new()
        } else if life {
            format!("{} / {}", p.life.max(0), p.max_life)
        } else if weapon {
            format!("{}\n{ammo}", if usp { "USP" } else { "M9" })
        } else if it {
            item.clone().unwrap_or_default()
        } else if pr {
            prompt.clone()
        } else if centre {
            if p.dead() { "GAME OVER\n[Enter] continue".into() } else { String::new() }
        } else if timer {
            game.timer.as_ref().map(|t| t.text()).unwrap_or_default()
        } else {
            continue;
        };
        set_text(&mut texts, e, s);
    }
}
