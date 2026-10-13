//! Prints a `.mar` motion archive's motions: id, flags, step, length and
//! the header words of the chosen ones (`mar_info <file.mar> [index...]`).

use mgs2_formats::mar;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: mar_info <file.mar> [index...]");
    let data = std::fs::read(&path).expect("read");
    let m = mar::parse(&data).expect("parse");
    let pick: Vec<usize> = args.filter_map(|a| a.parse().ok()).collect();
    let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    let base = u32_at(0xC) as usize;
    for (i, mo) in m.motions.iter().enumerate() {
        if !pick.is_empty() && !pick.contains(&i) {
            continue;
        }
        println!("{i:3}: id {:#06x} flags {:#x} step {:#x} length {}", mo.id, mo.flags, mo.step, mo.length);
        if !pick.is_empty() {
            let at = base + u32_at(0x10 + i * 8) as usize;
            let words: Vec<String> = (0..12).map(|k| format!("{:#x}", u32_at(at + k * 4))).collect();
            println!("     header {}", words.join(" "));
        }
    }
}
