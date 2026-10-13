//! Lists a stage's files (`stage_file <iso> <stage>`), or writes one out
//! (`stage_file <iso> <stage> <name>.<ext> <out>`).

use mgs2_formats::str_code;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (iso, stage) = (&a[0], &a[1]);
    let f = mgs2_formats::iso::Iso::open(std::path::Path::new(iso)).expect("iso").open_file("MGS2/STAGE.DAT").expect("STAGE.DAT");
    let mut dat = mgs2_formats::stage_dat::StageDat::open(f).expect("stage.dat");
    let st = dat.read_stage(stage).expect("stage");
    let names = mgs2_formats::names::Names::with_builtin();
    match a.get(2) {
        None => {
            for (_, fe) in st.files() {
                println!("{}.{} {}", names.show(fe.name_hash()), fe.extension(), fe.size);
            }
        }
        Some(want) => {
            let (name, ext) = want.rsplit_once('.').expect("name.ext");
            let hash = if let Some(h) = name.strip_prefix('#') { u32::from_str_radix(h, 16).unwrap() } else { str_code(name.as_bytes()) };
            let (page, fe) = st.files().find(|(_, f)| f.name_hash() == hash & 0xFF_FFFF && f.extension() == ext).expect("no such file");
            std::fs::write(&a[3], page.file_data(fe)).unwrap();
            println!("{} bytes", fe.size);
        }
    }
}
