//! `.tri` texture sets (file type 19, letter `t`).
//!
//! Loader: `FUN_00129d70` / `FUN_0012af10` / `FUN_0012b068`. The file is a
//! VRAM image the game DMAs to the GS as PSMCT32, 64 pixels wide:
//!
//! ```text
//! +00 u32 0
//! +04 u32 image rows   (rows of 64 PSMCT32 pixels = 256 bytes each)
//! +08 u32 image rows (again)
//! +0C u32 clut rows
//! +10 u32 texture count
//! +14 u32 flags        (bit 1: allocate in the shared VRAM area)
//! +18 u32 image data offset
//! +1C u32 clut data offset
//! +20 count x 0xA0-byte entries
//! ```
//!
//! Each entry holds a name hash at +0x10 and, from +0x20, a GIF packet of A+D
//! register writes (CLAMP_1, TEX2_1, TEX0_1, ALPHA_1; ALPHA_1 gives the
//! blend, [`Blend`]). TEX0's TBP0 is relative
//! to the image upload and CBP to the clut upload (which follows the image).
//! Textures sharing a file are packed into one atlas; CLAMP_1's region clamp
//! gives each one's rectangle.

use crate::gs::{GsMem, PSMT4, PSMT8, Tex0, expand_alpha};
use crate::{Result, format_err};

const ENTRY_SIZE: usize = 0xA0;

#[derive(Clone, Debug)]
pub struct Texture {
    /// `str_code` of the texture name; models refer to textures by this.
    pub hash: u32,
    pub tex0: Tex0,
    /// Region clamp rectangle within the atlas: min/max u, v (inclusive).
    pub rect: [u32; 4],
    /// Four floats at entry+0; looks like the rect in normalised atlas UVs.
    pub uv: [f32; 4],
    pub width: u32,
    pub height: u32,
    /// RGBA8, `width * height * 4` bytes, alpha expanded to 0..=255.
    pub rgba: Vec<u8>,
    /// How it blends with what is drawn (its ALPHA_1).
    pub blend: Blend,
}

/// The GS blend an entry's ALPHA_1 sets up, `(A - B) * C + D` with A, B, D
/// one of source, destination, zero and C source alpha, destination alpha
/// or a fixed value. In w04a: 326 normal, 11 + 6 additive (floor
/// markings), 19 subtractive (dark overlays laid over them), 2 that leave
/// the destination as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Blend {
    /// `(Cs - Cd) * As + Cd`, and anything not recognised.
    #[default]
    Normal,
    /// `Cs * As + Cd` (or by the destination's alpha).
    Add,
    /// `Cd - Cs * As`.
    Subtract,
    /// The destination unchanged (`C` fixed at 0).
    None,
}

impl Blend {
    pub fn from_alpha(alpha: u64) -> Blend {
        let (a, b, c, d, fix) = (alpha & 3, (alpha >> 2) & 3, (alpha >> 4) & 3, (alpha >> 6) & 3, (alpha >> 32) & 0xFF);
        match (a, b, c, d) {
            (0, 2, 0 | 1, 1) => Blend::Add,
            (2, 0, 0, 1) => Blend::Subtract,
            (_, _, 2, 1) if fix == 0 => Blend::None,
            _ => Blend::Normal,
        }
    }
}

pub fn parse(data: &[u8]) -> Result<Vec<Texture>> {
    if data.len() < 0x20 {
        return format_err("tri: file too small");
    }
    let rd = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    let (rows, clut_rows, count) = (rd(4), rd(0xC), rd(0x10) as usize);
    let (img_off, clut_off) = (rd(0x18) as usize, rd(0x1C) as usize);
    if 0x20 + count * ENTRY_SIZE > data.len() {
        return format_err("tri: entry table out of range");
    }
    let take = |off: usize, rows: u32| {
        let end = (off + rows as usize * 256).min(data.len());
        &data[off.min(end)..end]
    };

    let mut gs = GsMem::default();
    gs.upload32(0, 1, 64, take(img_off, rows));
    gs.upload32(rows, 1, 64, take(clut_off, clut_rows));

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = &data[0x20 + i * ENTRY_SIZE..0x20 + (i + 1) * ENTRY_SIZE];
        let q = |o: usize| u64::from_le_bytes(e[o..o + 8].try_into().unwrap());
        let mut tex0 = None;
        let mut clamp = 0u64;
        let mut alpha = 0x44u64;
        // A+D pairs follow the GIF tag at +0x20.
        for r in 0..5 {
            let (val, reg) = (q(0x30 + r * 16), q(0x38 + r * 16) & 0xFF);
            match reg {
                0x06 => tex0 = Some(Tex0::decode(val)),
                0x08 => clamp = val,
                0x42 => alpha = val,
                _ => {}
            }
        }
        let Some(mut t) = tex0 else {
            return format_err(format!("tri: entry {i} has no TEX0"));
        };
        t.cbp += rows;
        let fld = |s: u32| ((clamp >> s) & 0x3FF) as u32;
        let rect = if clamp & 0xF == 0xA {
            [fld(4), fld(14), fld(24), fld(34)]
        } else {
            [0, t.tw - 1, 0, t.th - 1]
        };
        let (w, h) = (rect[1] + 1 - rect[0], rect[3] + 1 - rect[2]);
        let clut = gs.clut32(t.cbp, t.psm, t.csa);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in rect[2]..=rect[3] {
            for x in rect[0]..=rect[1] {
                let idx = match t.psm {
                    PSMT4 => gs.read4(t.tbp, t.tbw, x, y),
                    PSMT8 => gs.read8(t.tbp, t.tbw, x, y),
                    other => return format_err(format!("tri: unsupported PSM {other:#x}")),
                };
                let c = clut[idx as usize];
                rgba.extend_from_slice(&[c[0], c[1], c[2], if t.tcc { expand_alpha(c[3]) } else { 255 }]);
            }
        }
        let uv = std::array::from_fn(|k| f32::from_le_bytes(e[k * 4..k * 4 + 4].try_into().unwrap()));
        out.push(Texture {
            hash: u32::from_le_bytes(e[0x10..0x14].try_into().unwrap()),
            tex0: t,
            rect,
            uv,
            width: w,
            height: h,
            rgba,
            blend: Blend::from_alpha(alpha),
        });
    }
    Ok(out)
}
