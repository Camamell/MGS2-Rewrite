//! Prints a `.kms` model's objects: parent, bind offset, bounding box and
//! meshes (vertex counts, textures).
//!
//! `cargo run -p mgs2-formats --example kms_info -- extracted/r_tnk0/p01/413aa8.kms`

use mgs2_formats::kms;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    assert!(!paths.is_empty(), "usage: kms_info <file.kms>...");
    for path in &paths {
        let Ok(m) = kms::parse(&std::fs::read(path).expect("read")) else { continue };
        if paths.len() > 1 {
            // Several files: only objects with unusual flags.
            for (i, o) in m.objects.iter().enumerate().filter(|(_, o)| o.flags != 1) {
                let p = usize::try_from(o.parent).ok().map(|p| m.objects[p].offset);
                println!("{path}: {i} parent {} flags {:#x} offset {:?} parent offset {:?}", o.parent, o.flags, o.offset, p);
            }
            continue;
        }
        show(&m);
    }
}

fn show(m: &kms::Model) {
    println!("origin {:?}, {} objects", m.origin, m.objects.len());
    for (i, o) in m.objects.iter().enumerate() {
        let verts: Vec<String> = o.meshes.iter().map(|x| format!("{}v tex {:x?}", x.positions.len(), x.textures)).collect();
        println!(
            "{i:2}: parent {:2} flags {:#x} offset ({:.0}, {:.0}, {:.0}) bbox {:?} meshes [{}]",
            o.parent,
            o.flags,
            o.offset[0],
            o.offset[1],
            o.offset[2],
            o.bbox.map(|v| v as i32),
            verts.join(", ")
        );
    }
}
