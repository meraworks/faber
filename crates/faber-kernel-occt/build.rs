use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=src/bridge.cpp");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rustc-env=FABER_ADAPTER_HASH={}", adapter_hash());
    println!("cargo:rerun-if-env-changed=FABER_OCCT_SDK");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let packages = root.join(".deps/occt-sdk");
    let sdk = env::var_os("FABER_OCCT_SDK")
        .map(PathBuf::from)
        .unwrap_or_else(|| packages.join("opencascade-7.9.3-vc14-64"));
    let windows = env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows";
    let mut bridge = cc::Build::new();
    bridge
        .cpp(true)
        .file("src/bridge.cpp")
        .std("c++17")
        .debug(false)
        .warnings(false);
    if windows {
        assert!(
            sdk.join("inc/TopoDS_Shape.hxx").exists(),
            "OCCT SDK is missing. Set FABER_OCCT_SDK"
        );
        bridge
            .include(sdk.join("inc"))
            .flag("/EHsc")
            .flag("/MD")
            .define("_ITERATOR_DEBUG_LEVEL", "0")
            .define("NOMINMAX", None);
        println!(
            "cargo:rustc-link-search=native={}",
            sdk.join("win64/vc14/lib").display()
        );
    } else {
        let prefix = env::var_os("FABER_OCCT_SDK")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr"));
        bridge.include(prefix.join("include/opencascade"));
        println!(
            "cargo:rustc-link-search=native={}",
            prefix.join("lib").display()
        );
    }
    bridge.compile("faber_geometry_bridge");
    for library in [
        "TKernel",
        "TKMath",
        "TKG2d",
        "TKG3d",
        "TKGeomBase",
        "TKBRep",
        "TKGeomAlgo",
        "TKTopAlgo",
        "TKPrim",
        "TKBO",
        "TKBool",
        "TKMesh",
        "TKShHealing",
        "TKXSBase",
        "TKDESTEP",
        "TKDE",
        "TKFillet",
        "TKOffset",
        "TKHLR",
        "TKBin",
    ] {
        println!("cargo:rustc-link-lib=dylib={library}");
    }
    if !windows {
        return;
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let target = output.ancestors().nth(3).unwrap();
    for entry in fs::read_dir(sdk.join("win64/vc14/bin")).unwrap() {
        let source = entry.unwrap().path();
        if source.extension().is_some_and(|e| e == "dll") {
            copy_runtime(&source, target);
        }
    }
    for relative in [
        "3rdparty-vc14-64/tbb-2021.13.0-x64/bin/tbb12.dll",
        "3rdparty-vc14-64/tbb-2021.13.0-x64/bin/tbbmalloc.dll",
        "3rdparty-vc14-64/jemalloc-vc14-64/bin/jemalloc.dll",
    ] {
        copy_runtime(&packages.join(relative), target);
    }
    for relative in [
        "ffmpeg-3.3.4-64/bin",
        "freeimage-3.18.0-x64/bin",
        "freetype-2.13.3-x64/bin",
        "openvr-1.14.15-64/bin/win64",
    ] {
        for entry in fs::read_dir(packages.join("3rdparty-vc14-64").join(relative)).unwrap() {
            let source = entry.unwrap().path();
            if source.extension().is_some_and(|e| e == "dll") {
                copy_runtime(&source, target);
            }
        }
    }
}

// Hashes the adapter source, C++ bridge included. A change in the adapter changes the cache version.
fn adapter_hash() -> String {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut files: Vec<PathBuf> = fs::read_dir(root.join("src"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    let mut hash = Sha256::new();
    for file in files {
        hash.update(file.file_name().unwrap().to_string_lossy().as_bytes());
        hash.update([0u8]);
        hash.update(fs::read(&file).unwrap());
        hash.update([0u8]);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn copy_runtime(source: &Path, target: &Path) {
    let bytes = fs::read(source).unwrap();
    for directory in [
        target.to_path_buf(),
        target.join("deps"),
        target.join("examples"),
    ] {
        fs::create_dir_all(&directory).unwrap();
        let destination = directory.join(source.file_name().unwrap());
        if fs::read(&destination).is_ok_and(|existing| existing == bytes) {
            continue;
        }
        fs::copy(source, destination)
            .unwrap_or_else(|e| panic!("Cannot copy {}: {e}", source.display()));
    }
}
