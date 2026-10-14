//! Counts the type-6 effect constructor ids (`+0x10`) in DEMO.DAT's
//! cutscenes, to see which effects the port does not draw yet.
//! `cargo run --release -p mgs2-formats --example demo_effects -- <iso>`

use std::collections::BTreeMap;
use std::io::Read;

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

fn main() {
    let iso = std::env::args().nth(1).expect("usage: demo_effects <iso>");
    let mut f = mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso").open_file("MGS2/DEMO.DAT").expect("DEMO.DAT");
    let mut data = Vec::new();
    f.read_to_end(&mut data).expect("read");
    let mut counts: BTreeMap<u32, (usize, Vec<usize>)> = BTreeMap::new();
    let mut sector = 0;
    while sector * 2048 < data.len() {
        let start = sector * 2048;
        if u32_at(&data, start) != Some(0x10) {
            sector += 1;
            continue;
        }
        let len = mgs2_formats::demo::extent(&data[start..]);
        let demo = &data[start..start + len];
        let mut pos = 0;
        while let (Some(kind), Some(size)) = (u32_at(demo, pos), u32_at(demo, pos + 4)) {
            let size = size as usize;
            if size < 16 || pos + size > demo.len() {
                break;
            }
            if kind == 2 {
                let chunk = &demo[pos..pos + size];
                let mut p = 16;
                while let (Some(t), Some(sz)) = (u32_at(chunk, p), u32_at(chunk, p + 4)) {
                    let sz = sz as usize;
                    if sz < 8 || p + sz > chunk.len() {
                        break;
                    }
                    if t & 0x7FFF_FFFF == 6
                        && let Some(id) = u32_at(chunk, p + 0x10)
                    {
                        let e = counts.entry(id).or_default();
                        e.0 += 1;
                        if !e.1.contains(&sector) && e.1.len() < 4 {
                            e.1.push(sector);
                        }
                    }
                    p += sz;
                }
            }
            pos += size;
        }
        sector += len.div_ceil(2048).max(1);
    }
    for (id, (n, demos)) in counts {
        println!("{id:#010x} x{n} (demos at sectors {demos:?})");
    }
}
