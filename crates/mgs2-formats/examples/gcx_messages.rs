//! Lists the actor messages (`#3822c7 <actor> <code>`) a stage script sends,
//! grouped by the kind of actor (`chara <kind> <name>`) they go to.
//! `cargo run --release -p mgs2-formats --example gcx_messages -- <iso> <stage>`

use std::collections::BTreeMap;

use mgs2_formats::gcx::{self, Stmt, Value};
use mgs2_formats::str_code;

fn walk(stmts: &[Stmt], kinds: &mut BTreeMap<u32, u32>, sent: &mut Vec<(u32, u32)>) {
    let chara = str_code(b"chara");
    for s in stmts {
        if let Stmt::Command { name, args, options } = s {
            let n = |i: usize| match args.get(i) {
                Some(Value::Name(h)) => Some(*h & 0xFF_FFFF),
                Some(Value::Int(_, v)) => Some(*v as u32 & 0xFF_FFFF),
                _ => None,
            };
            if *name == chara
                && let (Some(k), Some(a)) = (n(0), n(1))
            {
                kinds.insert(a, k);
            }
            if *name & 0xFF_FFFF == 0x3822C7
                && let (Some(t), Some(c)) = (n(0), n(1))
            {
                sent.push((t, c));
            }
            for v in args.iter().chain(options.iter().flat_map(|(_, v)| v)) {
                val(v, kinds, sent);
            }
        }
    }
}

fn val(v: &Value, kinds: &mut BTreeMap<u32, u32>, sent: &mut Vec<(u32, u32)>) {
    match v {
        Value::Block(b) => walk(b, kinds, sent),
        Value::Expr(items) => {
            for i in items {
                if let gcx::ExprItem::Push(v) = i {
                    val(v, kinds, sent);
                }
            }
        }
        _ => {}
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let f = mgs2_formats::iso::Iso::open(std::path::Path::new(&a[0])).expect("iso").open_file("MGS2/STAGE.DAT").expect("STAGE.DAT");
    let mut dat = mgs2_formats::stage_dat::StageDat::open(f).expect("stage.dat");
    let st = dat.read_stage(&a[1]).expect("stage");
    let (page, fe) = st.files().find(|(_, f)| f.type_id() == 6).expect("script");
    let g = gcx::parse(page.file_data(fe)).expect("gcx");
    let (mut kinds, mut sent) = (BTreeMap::new(), Vec::new());
    walk(&g.main, &mut kinds, &mut sent);
    for p in &g.procs {
        walk(&p.body, &mut kinds, &mut sent);
    }
    let mut by: BTreeMap<String, BTreeMap<u32, usize>> = BTreeMap::new();
    for (t, c) in sent {
        let k = kinds.get(&t).map_or("?".to_string(), |k| format!("#{k:06x}"));
        *by.entry(k).or_default().entry(c).or_default() += 1;
    }
    for (k, codes) in by {
        println!("{k}: {codes:?}");
    }
}
