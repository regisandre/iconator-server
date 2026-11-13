#![allow(
    clippy::large_const_arrays,
    reason = "generated lookup tables, only read once at build time"
)]

use std::{env, fs, path::Path};

use fst::Map;

include!("./icons/lookup_table.rs");

fn main() {
    // A build script that emits `rerun-if-changed` is only rerun for the listed paths (and for
    // changes to itself), so the table has to be listed or editing it would not rebuild the maps.
    println!("cargo::rerun-if-changed=icons/lookup_table.rs");

    let out_dir = env::var_os("OUT_DIR").expect("cargo sets OUT_DIR");
    let out_dir = Path::new(&out_dir);
    write_map(out_dir, "ext_icons.bin", &EXT_ICONS);
    write_map(out_dir, "filename_icons.bin", &FILENAME_ICONS);
    write_map(out_dir, "folder_icons.bin", &FOLDER_ICONS);
}

/// Writes `entries` as an fst map keyed by the lowercased name, which is how `src/lib.rs` looks
/// names up. fst needs sorted, unique keys, so identical pairs are merged and a key mapped to two
/// different icons fails the build.
fn write_map(out_dir: &Path, file_name: &str, entries: &[(&str, u64)]) {
    let mut entries: Vec<(String, u64)> = entries
        .iter()
        .map(|&(key, icon)| (key.to_ascii_lowercase(), icon))
        .collect();
    entries.sort_unstable();
    entries.dedup();
    if let Some(pair) = entries.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        panic!(
            "{file_name}: `{}` maps to icons {} and {}",
            pair[0].0, pair[0].1, pair[1].1
        );
    }

    let map = Map::from_iter(entries).expect("keys are sorted and unique");
    fs::write(out_dir.join(file_name), map.as_fst().as_bytes()).expect("OUT_DIR is writable");
}
