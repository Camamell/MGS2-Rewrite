//! Prints one object's meshes of a `.kms` model: flags, textures and
//! vertex positions. `kms_mesh <file.kms> <object>`
use mgs2_formats::kms;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = kms::parse(&std::fs::read(&a[0]).expect("read")).expect("kms");
    let o = &m.objects[a[1].parse::<usize>().expect("object")];
    for (i, x) in o.meshes.iter().enumerate() {
        println!("mesh {i}: flags {:#x} tex {:x?} indices {:?}", x.flags, x.textures, x.indices);
        for p in &x.positions {
            println!("  {:.0} {:.0} {:.0}", p[0], p[1], p[2]);
        }
    }
}
