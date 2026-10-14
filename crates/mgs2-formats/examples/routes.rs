//! Prints a `.hzx` file's patrol routes.
//! `cargo run -p mgs2-formats --example routes -- file.hzx [route]`
//! (`traps` in place of the route lists the named trap boxes instead;
//! `material <n>` the floors of that material).

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: routes <file.hzx> [route]");
    let only: Option<usize> = args.next().and_then(|s| s.parse().ok());
    let data = std::fs::read(path).expect("read");
    let hzx = mgs2_formats::hzx::parse(&data).expect("parse");
    // `material <n>`: the floors whose flags >> 28 is n (their centres).
    if std::env::args().nth(2).as_deref() == Some("material") {
        let n: u32 = std::env::args().nth(3).and_then(|v| v.parse().ok()).unwrap_or(0);
        for f in hzx.areas.iter().flat_map(|a| &a.groups).flat_map(|g| &g.floors).filter(|f| f.flags >> 28 == n) {
            for p in &f.polygons {
                let c = p.vertices.iter().fold([0.0f32; 3], |a, v| [a[0] + v[0], a[1] + v[1], a[2] + v[2]]);
                let k = p.vertices.len() as f32;
                println!("{:.0} {:.0} {:.0}", c[0] / k, c[1] / k, c[2] / k);
            }
        }
        return;
    }
    if std::env::args().nth(2).as_deref() == Some("traps") {
        for t in &hzx.traps {
            println!("#{:06x} {:?} .. {:?}", t.name & 0xFF_FFFF, t.min, t.max);
        }
        return;
    }
    for (i, r) in hzx.routes.iter().enumerate().filter(|(i, _)| only.is_none_or(|o| o == *i)) {
        println!("route {i}: {} points", r.points.len());
        for (n, p) in r.points.iter().enumerate() {
            println!(
                "  {n:2} ({:7.0} {:7.0} {:7.0}) wait {:5} dir {:5} flags {:#06x} action {} look {:?}",
                p.position[0], p.position[1], p.position[2], p.wait, p.direction, p.flags, p.action, p.look_at
            );
        }
    }
}
