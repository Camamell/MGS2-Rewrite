//! Prints the `str_code` hash of each argument (`str_code w02a snake ...`).
fn main() {
    for a in std::env::args().skip(1) {
        println!("{a}: {:#08x}", mgs2_formats::str_code(a.as_bytes()));
    }
}
