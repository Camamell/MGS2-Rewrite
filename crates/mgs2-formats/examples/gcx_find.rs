//! Prints every command in a stage script whose first argument (or name) is
//! a given `str_code`, with the proc it sits in, to see how scripts use it.
//! `cargo run -p mgs2-formats --example gcx_find -- <iso> <stage> <id hex> [proc]`

use mgs2_formats::gcx::{self, Stmt, Value};

fn walk(stmts: &[Stmt], want: u32, at: &str) {
    for s in stmts {
        if let Stmt::Call { proc, args } = s
            && *proc & 0xFF_FFFF == want
        {
            println!("{at}: call #{want:06x} {args:?}");
        }
        if let Stmt::Command { name, args, options } = s {
            let hit = *name & 0xFF_FFFF == want || matches!(args.first(), Some(Value::Name(h)) if *h & 0xFF_FFFF == want);
            if hit {
                println!("{at}: #{:06x} {args:?} {options:?}", name & 0xFF_FFFF);
            }
            for v in args.iter().chain(options.iter().flat_map(|(_, v)| v)) {
                val(v, want, at);
            }
        }
    }
}

fn val(v: &Value, want: u32, at: &str) {
    match v {
        Value::Expr(items) => {
            for i in items {
                if let gcx::ExprItem::Push(v) = i {
                    val(v, want, at);
                }
            }
        }
        Value::Block(b) => walk(b, want, at),
        _ => {}
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iso = args.next().expect("usage: gcx_find <iso> <stage> <id hex>");
    let stage = args.next().expect("stage");
    let want = u32::from_str_radix(args.next().expect("id").trim_start_matches('#'), 16).expect("hex");
    let f = mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso").open_file("MGS2/STAGE.DAT").expect("STAGE.DAT");
    let mut dat = mgs2_formats::stage_dat::StageDat::open(f).expect("stage.dat");
    let st = dat.read_stage(&stage).expect("stage");
    let (page, fe) = st.files().find(|(_, f)| f.type_id() == 6).expect("script");
    let g = gcx::parse(page.file_data(fe)).expect("gcx");
    walk(&g.main, want, "main");
    // A fourth argument `proc`: print the whole body of proc `id` instead.
    if args.next().as_deref() == Some("proc") {
        for p in g.procs.iter().filter(|p| p.id & 0xFF_FFFF == want) {
            println!("{:#?}", p.body);
        }
        return;
    }
    for p in &g.procs {
        walk(&p.body, want, &format!("proc #{:06x}", p.id));
    }
}
