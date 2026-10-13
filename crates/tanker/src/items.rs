//! Item boxes (`chara 0x9aff54 <name> -p x y z -k model -f kind -i id -n
//! count -x { on pickup }`) and Snake's inventory.
//!
//! `-f` is the category: 0 item, 1 weapon ammunition, 2 grenade-type
//! weapon. Names follow MGS2's item and weapon order.

use std::collections::{BTreeMap, HashSet};

use bevy::prelude::*;

use crate::body::Body;
use crate::gcl::Spawn;
use crate::player::Player;

pub const ITEM_ACTOR: u32 = 0x9AFF54;
/// Inventory key of the M9's darts (weapon ammunition, weapon 1).
pub const M9_AMMO: (i32, i32) = (1, 1);
/// Inventory key of dog tags (item 33).
pub const DOG_TAGS: (i32, i32) = (0, 33);
/// Inventory key of the USP's rounds (weapon 2).
pub const USP_AMMO: (i32, i32) = (1, 2);
/// Rounds in the USP Snake takes from Olga: none (w00c's script sets
/// Bs16:0x154[2] = 0 as he arrives from her defeat; USP ammunition lies in
/// the stages from w01b on).
pub const USP_START: i32 = 0;
/// Rounds the `MGS2_USP` test hook starts with.
const TEST_USP_ROUNDS: i32 = 15;

/// Snake's ammunition as the stage scripts read it: Bs16:0x154[w] per
/// weapon (1 the M9, 2 the USP; -1 = not carried).
pub const WEAPON_AMMO: u16 = 0x154;

impl Inventory {
    /// Mirrors the ammunition into the script variables.
    pub fn to_banks(&self, banks: &mut crate::gcl::Banks) {
        let m9 = self.counts.get(&M9_AMMO).copied().unwrap_or(0);
        let usp = if self.usp { self.counts.get(&USP_AMMO).copied().unwrap_or(0) } else { -1 };
        banks.set_b16(WEAPON_AMMO + 2, m9 as i16);
        banks.set_b16(WEAPON_AMMO + 4, usp as i16);
        // Items Snake carries (Bs16:0x1e4[id]): the stages leave out the
        // goggles' and boxes' item boxes once he has them, and add rations
        // when he has none.
        for id in TRACKED_ITEMS {
            let n = self.counts.get(&(0, id)).copied().unwrap_or(0);
            banks.set_b16(ITEMS + 2 * id as u16, n.clamp(0, i16::MAX as i32) as i16);
        }
    }
}

/// Snake's items as the scripts read them: Bs16:0x1e4[id].
pub const ITEMS: u16 = 0x1E4;
/// The items the port keeps (ration, cold medicine, bandage, thermal
/// goggles, the boxes, dog tags).
const TRACKED_ITEMS: [i32; 7] = [1, 4, 5, 13, 16, 24, 33];

/// Difficulty (10..50) as an index 0..4.
fn level(difficulty: i16) -> usize {
    (difficulty / 10 - 1).clamp(0, 4) as usize
}

/// Darts Snake starts with: the title script's new Tanker game sets
/// Bs16:0x154[1] to As16:0x7f0 (106 / 46 / 16 / 16 / 16 by difficulty).
pub fn m9_start(difficulty: i16) -> i32 {
    [106, 46, 16, 16, 16][level(difficulty)]
}

/// The most of each ammunition Snake can carry (the title script's
/// Bs16:0x19c table by difficulty), if capped.
pub fn max_count(key: (i32, i32), difficulty: i16) -> Option<i32> {
    let l = level(difficulty);
    match key {
        M9_AMMO => Some([106, 106, 46, 46, 46][l]),
        USP_AMMO => Some([61, 61, 46, 31, 16][l]),
        (2, 10) | (2, 11) => Some([50, 40, 20, 10, 6][l]),
        _ => None,
    }
}
/// Pickup reach (mm, horizontal) and height tolerance.
const REACH: f32 = 700.0;

#[derive(Component, Clone, Debug)]
pub struct Item {
    /// The actor's name (first argument), remembered once taken.
    pub name: u32,
    pub kind: i32,
    pub id: i32,
    pub count: i32,
    pub pos: Vec3,
}

impl Item {
    pub fn from_spawn(s: &Spawn) -> Option<(Item, u32)> {
        let name = s.args.first()?.int()? as u32;
        let p = |i| s.opt_int(b'p', i).unwrap_or(0) as f32;
        let model = s.opt(b'k')?.first()?.int()? as u32 & 0x00FF_FFFF;
        let item = Item {
            name,
            kind: s.opt_int(b'f', 0).unwrap_or(0),
            id: s.opt_int(b'i', 0).unwrap_or(0),
            count: s.opt_int(b'n', 0).unwrap_or(1),
            pos: Vec3::new(p(0), p(1), p(2)),
        };
        Some((item, model))
    }
}

pub fn name(kind: i32, id: i32) -> String {
    let known = match (kind, id) {
        (0, 1) => "Ration",
        (0, 4) => "Cold Medicine",
        (0, 5) => "Bandage",
        (0, 13) => "Thermal Goggles",
        (0, 16) | (0, 24) => "Box",
        (0, 33) => "Dog Tags",
        (1, 1) => "M9 darts",
        (1, 2) => "USP ammo",
        (2, 10) => "Chaff G.",
        (2, 11) => "Stun G.",
        (2, 17) => "Grenade",
        _ => "",
    };
    if known.is_empty() { format!("item {kind}/{id}") } else { known.to_string() }
}

/// What Snake carries, kept across stages.
#[derive(Resource)]
pub struct Inventory {
    pub counts: BTreeMap<(i32, i32), i32>,
    /// Item actors already taken, per stage.
    pub taken: HashSet<(String, u32)>,
    /// Last pickup message and seconds to show it.
    pub message: Option<(String, f32)>,
    pub ration_equipped: bool,
    /// Has the USP (from Olga).
    pub usp: bool,
    /// Weapon in hand: 0 the M9, 1 the USP.
    pub weapon: u8,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new(30)
    }
}

impl Inventory {
    /// A new game's inventory for the difficulty.
    pub fn new(difficulty: i16) -> Self {
        // MGS2_USP=1: start with the USP in hand (testing).
        let usp = std::env::var_os("MGS2_USP").is_some();
        let mut counts = BTreeMap::from([(M9_AMMO, m9_start(difficulty))]);
        if usp {
            counts.insert(USP_AMMO, TEST_USP_ROUNDS);
        }
        Self {
            counts,
            taken: HashSet::new(),
            message: None,
            ration_equipped: false,
            usp,
            weapon: u8::from(usp),
        }
    }
}

pub fn pickup(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    game: Res<crate::Game>,
    mut inv: ResMut<Inventory>,
    mut snake: Query<(&Body, &mut Player)>,
    items: Query<(Entity, &Item)>,
) {
    if let Some((_, t)) = inv.message.as_mut() {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            inv.message = None;
        }
    }
    let Ok((body, mut player)) = snake.single_mut() else { return };
    if keys.just_pressed(KeyCode::KeyR) {
        inv.ration_equipped = !inv.ration_equipped;
    }
    for (e, item) in &items {
        let d = Vec2::new(item.pos.x - body.pos.x, item.pos.z - body.pos.z).length();
        if d > REACH || (item.pos.y - body.pos.y).abs() > 1000.0 || player.dead() {
            continue;
        }
        let key = (item.kind, item.id);
        let max = max_count(key, game.banks.b16(crate::gcl::DIFFICULTY));
        let have = inv.counts.entry(key).or_default();
        *have = (*have + item.count).min(max.unwrap_or(i32::MAX));
        inv.taken.insert((game.data.name.clone(), item.name));
        inv.message = Some((format!("{} x{}", name(item.kind, item.id), item.count), 2.5));
        commands.entity(e).despawn();
        // item_box.c: 0x16 when an item is taken.
        crate::sfx::play(crate::sfx::PICKUP, Some(item.pos));
    }
    // Rations live in the inventory; the revive uses an equipped one.
    player.rations = if inv.ration_equipped { inv.counts.get(&(0, 1)).copied().unwrap_or(0).max(0) as u32 } else { 0 };
}

/// After a revive (`Player::damage` used one), take it from the inventory.
pub fn sync_rations(mut inv: ResMut<Inventory>, snake: Query<&Player>) {
    let Ok(p) = snake.single() else { return };
    if inv.ration_equipped {
        let have = inv.counts.get(&(0, 1)).copied().unwrap_or(0);
        if (p.rations as i32) < have {
            inv.counts.insert((0, 1), p.rations as i32);
            inv.message = Some(("Ration used".into(), 2.5));
        }
    }
}
