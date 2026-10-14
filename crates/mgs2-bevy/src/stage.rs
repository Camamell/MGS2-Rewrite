//! Loads a stage's data from the disc image into plain Rust structures.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mgs2_formats::iso::{Iso, SubFile};
use mgs2_formats::stage_dat::{StageDat, is_tanker_stage};
use mgs2_formats::gcx::{self, Stmt, Value};
use mgs2_formats::{hzx, kms, mar, str_code, tri};

pub type Disc = StageDat<SubFile<BufReader<File>>>;

pub fn open_disc(iso: &Path) -> Result<Disc> {
    let file = Iso::open(iso).with_context(|| format!("opening {}", iso.display()))?.open_file("MGS2/STAGE.DAT")?;
    Ok(StageDat::open(file)?)
}

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub blend: mgs2_formats::tri::Blend,
}

/// Actor type that places static level geometry:
/// `chara 0x35a2cf <instance> -k <model> x y z -h <hzx> <area> -l <lt2>`.
pub const MAP_PIECE: u32 = 0x35A2CF;

/// A level-geometry model placed by the stage script.
#[derive(Clone, Debug)]
pub struct MapPiece {
    pub model: u32,
    /// Millimetres.
    pub position: [f32; 3],
    /// Collision area index (`-h <hzx> <area>`); not used by the viewer yet.
    #[allow(dead_code)]
    pub hzx_area: Option<i32>,
    /// Its lighting (`-l <lt2>`).
    pub lt2: Option<u32>,
}

/// A player entry point set by a stage-script proc before Snake is created
/// (`chara 0x128946 ... -p Au32:0xbc Au32:0x7f4 Au32:0xc0 -d 0 Au32:0x7f8`).
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// Millimetres.
    pub position: [f32; 3],
    /// Facing, 4096 = full turn.
    pub direction: i32,
}

pub struct StageData {
    pub name: String,
    pub pieces: Vec<MapPiece>,
    pub entries: Vec<Entry>,
    /// Texture hash -> pixels, from every texture set in the stage.
    pub textures: HashMap<u32, Texture>,
    /// (file name hash, model), level geometry first.
    pub models: Vec<(u32, kms::Model)>,
    pub collision: Option<hzx::Hzx>,
    /// (file name hash, motion archive).
    pub motions: Vec<(u32, mar::Mar)>,
    /// (file name hash, motion sound events) for the `.mar` of that name.
    pub sars: Vec<(u32, mgs2_formats::sar::Sar)>,
    /// (file name hash, skinned cutscene mesh).
    pub evms: Vec<(u32, mgs2_formats::evm::Model)>,
    /// (file name hash, lighting).
    pub lights: Vec<(u32, mgs2_formats::lt2::Lt2)>,
    /// The stage script (`scenerio.gcx`).
    pub script: Option<gcx::Gcx>,
    /// The stage's resident code image (type-1 file, loaded at 0x1F1480):
    /// some actors' data tables are read from it.
    pub resident: Option<Vec<u8>>,
    /// The stage's sound pack (page `0x7F000010`, see `mgs2_formats::sd`).
    pub sound: Option<Vec<u8>>,
    /// The chapter resident's sound pack (common samples and effects),
    /// loaded before the stage's.
    pub resident_sound: Option<Vec<u8>>,
    pub warnings: Vec<String>,
}

impl StageData {
    /// Motion archives usable with a model, best match first: joint count
    /// closest to the model's object count (joints the model lacks, or
    /// objects the motion lacks, are left alone).
    pub fn motion_sets_for(&self, model: &kms::Model) -> Vec<usize> {
        let objects = model.objects.len();
        // Closest joint count first; among equals, the archive with the most
        // motions (a character's main set rather than a one-off scene).
        let mut v: Vec<(usize, std::cmp::Reverse<usize>, usize)> = self
            .motions
            .iter()
            .enumerate()
            .filter_map(|(i, (_, m))| {
                let joints = m.motions.iter().map(|x| x.joints.len()).max()?;
                (joints > 1).then(|| (objects.abs_diff(joints), std::cmp::Reverse(m.motions.len()), i))
            })
            .collect();
        v.sort();
        v.into_iter().map(|(_, _, i)| i).collect()
    }
}

/// Resident data kept loaded during the whole Tanker chapter: Snake, his
/// motions and weapons, menus, codec.
pub const TANKER_RESIDENT: &str = "r_tnk0";

pub fn load(disc: &mut Disc, name: &str) -> Result<StageData> {
    let stage = disc.read_stage(name)?;
    let resident = disc.read_stage(TANKER_RESIDENT)?;
    let mut data = StageData {
        name: name.to_string(),
        pieces: Vec::new(),
        entries: Vec::new(),
        textures: HashMap::new(),
        models: Vec::new(),
        collision: None,
        motions: Vec::new(),
        sars: Vec::new(),
        evms: Vec::new(),
        lights: Vec::new(),
        script: None,
        resident: None,
        sound: stage.pages.iter().find(|p| p.kind == 0x7F00_0010).map(|p| p.data.clone()),
        resident_sound: resident.pages.iter().find(|p| p.kind == 0x7F00_0010).map(|p| p.data.clone()),
        warnings: Vec::new(),
    };
    let level = str_code(name.as_bytes());
    // Stage files first so they win over resident ones with the same hash.
    let resident_files = resident.files().filter(|(_, f)| matches!(f.type_id(), 4 | 10 | 12 | 18 | 19));
    for (page, f) in stage.files().chain(resident_files) {
        let bytes = page.file_data(f);
        let what = format!("{:06x}.{}", f.name_hash(), f.extension());
        match f.type_id() {
            19 => match tri::parse(bytes) {
                Ok(set) => {
                    for t in set {
                        data.textures
                            .entry(t.hash)
                            .or_insert(Texture { width: t.width, height: t.height, rgba: t.rgba, blend: t.blend });
                    }
                }
                Err(e) => data.warnings.push(format!("{what}: {e}")),
            },
            10 if data.models.iter().all(|(h, _)| *h != f.name_hash()) => match kms::parse(bytes) {
                Ok(m) => data.models.push((f.name_hash(), m)),
                Err(e) => data.warnings.push(format!("{what}: {e}")),
            },
            7 if f.name_hash() == level => match hzx::parse(bytes) {
                Ok(h) => data.collision = Some(h),
                Err(e) => data.warnings.push(format!("{what}: {e}")),
            },
            12 if bytes.starts_with(b"MARa") && data.motions.iter().all(|(h, _)| *h != f.name_hash()) => match mar::parse(bytes) {
                Ok(m) => data.motions.push((f.name_hash(), m)),
                Err(e) => data.warnings.push(format!("{what}: {e}")),
            },
            18 if data.sars.iter().all(|(h, _)| *h != f.name_hash()) => match mgs2_formats::sar::parse(bytes) {
                Ok(s) => data.sars.push((f.name_hash(), s)),
                Err(e) => data.warnings.push(format!("{what}: {e}")),
            },
            4 if data.evms.iter().all(|(h, _)| *h != f.name_hash()) => match mgs2_formats::evm::parse(bytes) {
                Ok(m) => data.evms.push((f.name_hash(), m)),
                Err(e) => data.warnings.push(format!("{what}: {e}")),
            },
            11 if data.lights.iter().all(|(h, _)| *h != f.name_hash()) => match mgs2_formats::lt2::parse(bytes) {
                Ok(l) => data.lights.push((f.name_hash(), l)),
                Err(e) => data.warnings.push(format!("{what}: {e}")),
            },
            1 if data.resident.is_none() => data.resident = Some(bytes.to_vec()),
            6 if f.name_hash() == str_code(b"scenerio") => match gcx::parse(bytes) {
                Ok(g) => {
                    collect_pieces(&g.main, &mut data.pieces);
                    for p in &g.procs {
                        collect_entries(&p.body, &mut data.entries);
                    }
                    data.script = Some(g);
                }
                Err(e) => data.warnings.push(format!("{what}: {e}")),
            },
            _ => {}
        }
    }
    // The same model can be placed several times at one spot (once per
    // collision area); draw it once.
    let mut seen = Vec::new();
    data.pieces.retain(|p| {
        let key = (p.model, p.position.map(f32::to_bits));
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });
    if data.pieces.is_empty() && data.models.iter().any(|(h, _)| *h == level) {
        data.pieces.push(MapPiece { model: level, position: [0.0; 3], hzx_area: None, lt2: None });
    }
    // Level geometry first, then the rest by hash for a stable browse order.
    let is_level = |h: u32| data.pieces.iter().any(|p| p.model == h);
    let mut models = std::mem::take(&mut data.models);
    models.sort_by_key(|(h, _)| (!is_level(*h), *h));
    data.models = models;
    Ok(data)
}

/// How another stage's script sends Snake into `target`: a proc that sets
/// the start variables (and the story marker `Au32:0xc4`) and then runs
/// `load "<target>"`.
#[derive(Clone, Copy, Debug)]
pub struct Arrival {
    pub entry: Entry,
    pub progress: Option<u32>,
}

/// Just a stage's script (`scenerio.gcx`), without loading the rest.
pub fn script(disc: &mut Disc, name: &str) -> Result<Option<gcx::Gcx>> {
    let stage = disc.read_stage(name)?;
    let Some((page, f)) = stage.files().find(|(_, f)| f.type_id() == 6 && f.name_hash() == str_code(b"scenerio"))
    else {
        return Ok(None);
    };
    Ok(Some(gcx::parse(page.file_data(f))?))
}

pub fn arrivals(disc: &mut Disc, from: &str, target: &str) -> Result<Vec<Arrival>> {
    let Some(g) = script(disc, from)? else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for p in &g.procs {
        let loads_target = p.body.iter().any(|s| {
            matches!(s, Stmt::Command { name, args, .. }
                if *name == str_code(b"load") && matches!(args.first(), Some(Value::Str(t)) if t == target))
        });
        if !loads_target {
            continue;
        }
        let vars: std::collections::HashMap<u16, i32> = p.body.iter().filter_map(constant_assignment).collect();
        let progress = p.body.iter().find_map(|s| {
            let Stmt::Command { args, .. } = s else { return None };
            let [Value::Expr(items)] = args.as_slice() else { return None };
            match items.as_slice() {
                [gcx::ExprItem::Push(Value::Var(v)), gcx::ExprItem::Push(Value::Name(h)), gcx::ExprItem::Op(gcx::Op::Assign)]
                    if v.offset == 0xC4 && !v.bank_b =>
                {
                    Some(*h)
                }
                _ => None,
            }
        });
        if let (Some(x), Some(y), Some(z)) = (vars.get(&0xBC), vars.get(&0x7F4), vars.get(&0xC0)) {
            let entry = Entry {
                position: [*x as f32, *y as f32, *z as f32],
                direction: vars.get(&0x7F8).copied().unwrap_or(0),
            };
            out.push(Arrival { entry, progress });
        }
    }
    Ok(out)
}

/// `eval (A:var = constant)` -> (var offset, value).
fn constant_assignment(s: &Stmt) -> Option<(u16, i32)> {
    let Stmt::Command { name, args, .. } = s else { return None };
    if *name != str_code(b"eval") {
        return None;
    }
    let [Value::Expr(items)] = args.as_slice() else { return None };
    match items.as_slice() {
        [
            gcx::ExprItem::Push(Value::Var(v)),
            gcx::ExprItem::Push(Value::Int(_, x)),
            gcx::ExprItem::Op(gcx::Op::Assign),
        ] if !v.bank_b && v.index.is_none() => Some((v.offset, *x)),
        _ => None,
    }
}

/// Finds runs of assignments to the player start variables
/// (x 0xbc, y 0x7f4, z 0xc0, direction 0x7f8).
fn collect_entries(stmts: &[Stmt], out: &mut Vec<Entry>) {
    let assigns: Vec<Option<(u16, i32)>> = stmts.iter().map(constant_assignment).collect();
    for w in assigns.windows(4) {
        if let [Some((0xBC, x)), Some((0x7F4, y)), Some((0xC0, z)), Some((0x7F8, d))] = w {
            out.push(Entry { position: [*x as f32, *y as f32, *z as f32], direction: *d });
        }
    }
    for s in stmts {
        if let Stmt::Command { args, options, .. } = s {
            for v in args.iter().chain(options.iter().flat_map(|(_, v)| v)) {
                if let Value::Block(b) = v {
                    collect_entries(b, out);
                }
            }
        }
    }
}

/// Finds map-piece actors in the main script, including inside `if` blocks.
/// Pieces created later by procs (e.g. after events) are not included.
fn collect_pieces(stmts: &[Stmt], out: &mut Vec<MapPiece>) {
    for s in stmts {
        let Stmt::Command { name, args, options } = s else { continue };
        if *name == str_code(b"chara") && matches!(args.first(), Some(Value::Name(MAP_PIECE))) {
            let opt = |c: u8| options.iter().find(|(l, _)| *l == c).map(|(_, v)| v.as_slice());
            let int = |v: Option<&Value>| match v {
                Some(Value::Int(_, i)) => Some(*i),
                _ => None,
            };
            let area = opt(b'h').and_then(|h| int(h.get(1)));
            let lt2 = opt(b'l').and_then(|l| match l.first() {
                Some(Value::Name(h)) => Some(*h),
                Some(Value::Int(_, i)) => Some(*i as u32),
                _ => None,
            });
            // A piece may list several models, each `-k <model> [x y z]`.
            for (_, vals) in options.iter().filter(|(l, _)| *l == b'k') {
                if let [Value::Name(model), rest @ ..] = vals.as_slice() {
                    let p = [0, 1, 2].map(|i| int(rest.get(i)).unwrap_or(0) as f32);
                    out.push(MapPiece { model: *model, position: p, hzx_area: area, lt2 });
                }
            }
        }
        // Recurse into nested blocks (if/else bodies).
        for v in args.iter().chain(options.iter().flat_map(|(_, v)| v)) {
            if let Value::Block(b) = v {
                collect_pieces(b, out);
            }
        }
    }
}

/// Tanker stages with level geometry, in disc order (radio stages excluded).
pub fn tanker_stages(disc: &Disc) -> Vec<String> {
    disc.stages()
        .iter()
        .map(|s| s.name.clone())
        .filter(|n| is_tanker_stage(n) && !n.starts_with("r_"))
        .collect()
}

pub fn default_iso() -> Option<PathBuf> {
    std::env::var_os("MGS2_ISO").map(PathBuf::from)
}
