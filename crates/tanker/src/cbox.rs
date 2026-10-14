//! The cardboard box (`c_box.c`, items 16 / 24 "Box"): B puts it on or
//! takes it off. The game draws the worn box with the item's own model
//! (`box2_ibox`, `FUN_002ef740`); here that model is stretched over a
//! crouching Snake. Guards ignore a box that keeps still and only grow
//! suspicious ("?") of one that moves; one who walks into it (within 1.2 m,
//! dead ahead, or 0.8 m) looks it over and lifts it on Snake (`guard.rs`,
//! stimulus 0x20000; the inspection's own actions are not ported). Only in stages that carry the model (w00a-c, w01a-b).
//! `MGS2_BOX_TEST=1` wears it from the start.

use bevy::prelude::*;

use crate::body::Body;
use crate::player::Player;
use crate::{Game, SCALE};

const BOX_MODEL: &str = "box2_ibox";
/// The box items Snake may wear.
const BOXES: [(i32, i32); 2] = [(0, 16), (0, 24)];
/// Stretch of the item model (700 x 300 x 530 mm) to cover him.
const STRETCH: Vec3 = Vec3::new(1.15, 3.4, 1.4);

#[derive(Component)]
struct WornBox {
    stage: String,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (toggle.before(crate::player::control), show.after(crate::photo::view).after(crate::fpv::view)));
}

pub fn has_box(inv: &crate::items::Inventory) -> bool {
    BOXES.iter().any(|k| inv.counts.get(k).is_some_and(|n| *n > 0))
}

fn toggle(keys: Res<ButtonInput<KeyCode>>, game: Res<Game>, inv: Res<crate::items::Inventory>, mut player: Query<&mut Player>) {
    let Ok(mut p) = player.single_mut() else { return };
    // Only where the stage has the box's model to draw.
    let drawable = game.data.models.iter().any(|(h, _)| *h == mgs2_formats::str_code(BOX_MODEL.as_bytes()));
    let test = std::env::var_os("MGS2_BOX_TEST").is_some();
    if (keys.just_pressed(KeyCode::KeyB) && (has_box(&inv) || p.boxed)) || (test && !p.boxed) {
        p.boxed = !p.boxed;
    }
    if p.dead() || p.hang.is_some() || !drawable {
        p.boxed = false;
    }
    if p.boxed {
        p.crouch = true;
        p.prone = false;
        p.aiming = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    game: Res<Game>,
    mut player: Query<(&Body, &Player, &mut Visibility), Without<WornBox>>,
    mut boxes: Query<(Entity, &WornBox, &mut Transform, &mut Visibility), Without<Player>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok((b, p, mut vis)) = player.single_mut() else { return };
    if p.boxed {
        *vis = Visibility::Hidden;
    }
    let mut found = false;
    for (e, w, mut t, mut bv) in &mut boxes {
        if w.stage != game.data.name {
            commands.entity(e).try_despawn();
            continue;
        }
        found = true;
        *bv = if p.boxed { Visibility::Inherited } else { Visibility::Hidden };
        if p.boxed {
            t.translation = b.pos * SCALE;
            t.rotation = Quat::from_rotation_y(b.yaw);
            t.scale = STRETCH;
        }
    }
    if !found
        && let Some((_, model)) = game.data.models.iter().find(|(h, _)| *h == mgs2_formats::str_code(BOX_MODEL.as_bytes()))
    {
        let mut cache = mgs2_bevy::scene::MaterialCache::default();
        let e = mgs2_bevy::scene::spawn_model(&mut commands, model, &game.data, &mut cache, &mut meshes, &mut images, &mut materials, true, crate::LevelEntity);
        commands.entity(e).insert((WornBox { stage: game.data.name.clone() }, Visibility::Hidden));
    }
}
