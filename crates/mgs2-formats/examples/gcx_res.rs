//! Dumps a stage script's resource `n` (`str[n]`) as signed little-endian
//! words, for data blocks such as the enemy manager's `-v` voice table.
//! `cargo run -p mgs2-formats --example gcx_res -- <iso> <stage> <n>`

fn main() {
    let mut args = std::env::args().skip(1);
    let iso = args.next().expect("usage: gcx_res <iso> <stage> <n>");
    let stage = args.next().expect("stage");
    let n: usize = args.next().expect("n").parse().expect("number");
    let f = mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso").open_file("MGS2/STAGE.DAT").expect("STAGE.DAT");
    let mut dat = mgs2_formats::stage_dat::StageDat::open(f).expect("stage.dat");
    let st = dat.read_stage(&stage).expect("stage");
    let (page, fe) = st.files().find(|(_, f)| f.type_id() == 6).expect("script");
    let g = mgs2_formats::gcx::parse(page.file_data(fe)).expect("gcx");
    let r = &g.resources[n];
    for (i, w) in r.chunks_exact(4).enumerate() {
        let v = i32::from_le_bytes(w.try_into().unwrap());
        print!("{v:#010x} ");
        if i % 2 == 1 {
            println!();
        }
    }
    println!();
}
