use std::path::{Path, PathBuf};

use walkdir::WalkDir;

// Where the app finds libdelight_sdk.dylib and Rust's libstd at runtime:
// Contents/Frameworks in the app bundle (see scripts/bundle.sh), or next to
// the binary in a dev build. (`cargo run` itself adds the toolchain's libstd.)
fn main() {
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path/../Frameworks");
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path");
    embed_sdk_kit();
}

/// The SDK kit (see src/sdk_kit.rs): the workspace's manifest, lock file,
/// toolchain and flags, and the SDK's crates, embedded in the binary as
/// `&[(path, contents)]`, so the kit always matches this exact build.
fn embed_sdk_kit() {
    let root = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..").canonicalize().unwrap();
    let mut files: Vec<PathBuf> = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml"].map(PathBuf::from).into();
    for dir in ["crates/sdk", "crates/gpui-alias"] {
        println!("cargo:rerun-if-changed={}", root.join(dir).display());
        let walk = WalkDir::new(root.join(dir)).sort_by_file_name().into_iter();
        for entry in walk.filter_entry(|e| e.file_name() != "target").flatten().filter(|e| e.file_type().is_file()) {
            if entry.file_name() != ".DS_Store" {
                files.push(entry.path().strip_prefix(&root).unwrap().to_path_buf());
            }
        }
    }
    let mut list = String::from("&[\n");
    for file in &files {
        let path = root.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        list += &format!("    ({:?}, include_bytes!({:?})),\n", file.to_str().unwrap(), path.to_str().unwrap());
    }
    list += "]\n";
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("sdk_kit.rs"), list).unwrap();
}
