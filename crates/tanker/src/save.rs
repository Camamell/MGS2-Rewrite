//! Autosave and `--continue`. Each time a stage starts, the stage name, the
//! script variables as it started (both banks: story flags, Snake's start,
//! the holds' timer...) and the inventory are written to `save.txt` in the
//! user's data folder (`%APPDATA%\mgs2-tanker` on Windows). `--continue`
//! starts from it. (The game saves through the Codec; this is simpler.)

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::prelude::*;

use crate::gcl::Banks;
use crate::items::Inventory;
use crate::Game;

pub struct Save {
    pub stage: String,
    pub banks: Banks,
    pub counts: BTreeMap<(i32, i32), i32>,
    pub usp: bool,
    pub weapon: u8,
}

pub fn path() -> PathBuf {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME")).map_or_else(|| PathBuf::from("."), PathBuf::from);
    base.join("mgs2-tanker").join("save.txt")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

pub fn write(s: &Save) -> std::io::Result<()> {
    let counts: Vec<String> = s.counts.iter().map(|((k, i), n)| format!("{k},{i},{n}")).collect();
    let text = format!(
        "mgs2-tanker save 1\nstage {}\na {}\nb {}\nitems {}\nusp {} {}\n",
        s.stage,
        hex(&s.banks.a),
        hex(&s.banks.b),
        counts.join(";"),
        u8::from(s.usp),
        s.weapon
    );
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, text)
}

pub fn read() -> Option<Save> {
    let text = std::fs::read_to_string(path()).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "mgs2-tanker save 1" {
        return None;
    }
    let mut field = |name: &str| lines.next()?.strip_prefix(name)?.strip_prefix(' ').map(str::to_string);
    let stage = field("stage")?;
    let a = unhex(&field("a")?)?;
    let b = unhex(&field("b")?)?;
    let items = field("items").unwrap_or_default();
    let usp = field("usp").unwrap_or_default();
    let counts = items
        .split(';')
        .filter_map(|e| {
            let v: Vec<i32> = e.split(',').filter_map(|x| x.parse().ok()).collect();
            (v.len() == 3).then(|| ((v[0], v[1]), v[2]))
        })
        .collect();
    let mut u = usp.split(' ');
    let has = u.next() == Some("1");
    let weapon = u.next().and_then(|w| w.parse().ok()).unwrap_or(0);
    Some(Save { stage, banks: Banks { a, b }, counts, usp: has, weapon })
}

/// Saves after a call on Otacon's save frequency (140.96): the story as it
/// is now and the inventory; Snake starts the stage over from its entrance
/// when continuing.
pub fn codec_save(mut game: ResMut<Game>, inv: Res<Inventory>) {
    if !std::mem::take(&mut game.save_requested) {
        return;
    }
    let s = Save {
        stage: game.data.name.clone(),
        banks: Banks { a: game.banks.a.clone(), b: game.banks.b.clone() },
        counts: inv.counts.clone(),
        usp: inv.usp,
        weapon: inv.weapon,
    };
    // Keep the stage's entrance as the place to start from.
    let mut s = s;
    for off in [crate::gcl::SNAKE_X, crate::gcl::SNAKE_Y, crate::gcl::SNAKE_Z, crate::gcl::SNAKE_DIR] {
        s.banks.set_a32(off, game.start_banks.a32(off));
    }
    match write(&s) {
        Ok(()) => info!("saved through the Codec at {}", s.stage),
        Err(e) => warn!("save failed: {e}"),
    }
}

/// Writes the save when a new stage has started.
pub fn autosave(game: Res<Game>, inv: Res<Inventory>, mut last: Local<String>) {
    if *last == game.data.name {
        return;
    }
    *last = game.data.name.clone();
    let s = Save {
        stage: game.data.name.clone(),
        banks: Banks { a: game.start_banks.a.clone(), b: game.start_banks.b.clone() },
        counts: inv.counts.clone(),
        usp: inv.usp,
        weapon: inv.weapon,
    };
    match write(&s) {
        Ok(()) => info!("saved at {} ({})", s.stage, path().display()),
        Err(e) => warn!("save failed: {e}"),
    }
}
