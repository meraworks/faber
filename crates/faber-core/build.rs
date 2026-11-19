use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

// Hashes the source of this crate. A new build can change results, so cache keys change with it.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut files = Vec::new();
    for directory in [
        "src",
        "../faber-sketch/src",
        "../faber-sheet/src",
        "../faber-geometry/src",
    ] {
        println!("cargo:rerun-if-changed={directory}");
        collect(&root.join(directory), &mut files);
    }
    files.sort();
    let mut hash = Sha256::new();
    for file in files {
        hash.update(
            file.strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .as_bytes(),
        );
        hash.update([0u8]);
        hash.update(fs::read(&file).unwrap());
        hash.update([0u8]);
    }
    let digest: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    println!("cargo:rustc-env=FABER_CORE_BUILD={digest}");
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}
