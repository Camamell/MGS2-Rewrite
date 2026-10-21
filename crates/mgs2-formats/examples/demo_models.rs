//! Lists the model loads (type-4 packets) of DEMO.DAT demos: serial, name
//! hash, kms file id and evm id. With a hash (low 24 bits, hex) it lists
//! every demo that loads it as kms or evm instead.
//! `cargo run --release -p mgs2-formats --example demo_models -- <iso> <demo id hex>..`
//! `cargo run --release -p mgs2-formats --example demo_models -- <iso> find <hash hex>`

use std::io::Read;

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches("0x"), 16).expect("hex")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let iso = args.first().expect("usage: demo_models <iso> <id>.. | find <hash>");
    let mut f = mgs2_formats::iso::Iso::open(std::path::Path::new(iso)).expect("iso").open_file("MGS2/DEMO.DAT").expect("DEMO.DAT");
    let mut data = Vec::new();
    f.read_to_end(&mut data).expect("read");
    let show = |sector: usize| {
        let start = sector * 0x800;
        let len = mgs2_formats::demo::extent(&data[start..]);
        mgs2_formats::demo::parse(&data[start..start + len])
    };
    if args.get(1).map(String::as_str) == Some("find") {
        let want = hex(&args[2]) & 0x00FF_FFFF;
        let mut sector = 0;
        while sector * 0x800 < data.len() {
            if u32_at(&data, sector * 0x800) == Some(0x10) {
                let d = show(sector);
                for m in d.models.iter().filter(|m| m.file & 0x00FF_FFFF == want || (m.evm != 0 && m.evm & 0x00FF_FFFF == want)) {
                    println!("demo {sector:#x}: serial {} file {:#x} evm {:#x}", m.serial, m.file, m.evm);
                }
            }
            sector += 1;
        }
        return;
    }
    for id in &args[1..] {
        let d = show((hex(id) & 0x00FF_FFFF) as usize);
        println!("demo {id}: {} models", d.models.len());
        for m in &d.models {
            println!("  serial {:3} name {:#08x} file {:#010x} evm {:#010x} from {}", m.serial, m.name, m.file, m.evm, m.from);
        }
    }
}
