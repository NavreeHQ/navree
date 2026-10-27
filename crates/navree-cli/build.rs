//! Embeds every file under `templates/kits/` (the starter kits of
//! `navree new --kit`): writes `$OUT_DIR/kit_files.rs`, a sorted
//! `KIT_FILES: &[(&str, &str)]` of `(path relative to templates/kits, contents)`,
//! each contents an `include_str!` so the binary carries the kits.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let kits = manifest.join("templates").join("kits");
    // A directory: cargo rescans everything below it.
    println!("cargo:rerun-if-changed=templates/kits");

    let mut files = Vec::new();
    if kits.is_dir() {
        walk(&kits, &kits, &mut files);
    }
    files.sort();

    let mut out = String::from(
        "/// `(path under templates/kits, contents)` of every starter-kit file, sorted.\n\
         pub(crate) const KIT_FILES: &[(&str, &str)] = &[\n",
    );
    for (relative, absolute) in &files {
        writeln!(
            out,
            "    ({relative:?}, include_str!({:?})),",
            absolute.to_string_lossy()
        )
        .expect("writing to a String");
    }
    out.push_str("];\n");

    let target = PathBuf::from(std::env::var_os("OUT_DIR").expect("set by cargo")).join("kit_files.rs");
    std::fs::write(&target, out).expect("OUT_DIR is writable");
}

/// Every file below `dir`, as (`/`-separated path relative to `root`, absolute path).
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            walk(root, &path, out);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("below the root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((relative, path));
        }
    }
}
