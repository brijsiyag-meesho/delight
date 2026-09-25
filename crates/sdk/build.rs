use std::path::Path;

fn main() {
    // Records the compiler version for `BUILD_ID`: plugins must be built by the
    // same rustc as the app, since Rust has no stable ABI.
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    println!("cargo:rustc-env=DELIGHT_RUSTC_VERSION={}", version.trim());

    // A hash of the SDK's sources for `BUILD_ID`. Cargo's build identity (and
    // so the plugin checks) doesn't cover source contents: without it, a
    // plugin built before a layout change — a new field, enum variant or trait
    // method — would pass every check and then misread the app's types.
    println!("cargo:rustc-env=DELIGHT_SDK_SOURCE_HASH={:016x}", source_hash(Path::new("src")));
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");

    // A dylib keeps all of GPUI (nothing is dead-stripped), including
    // `io-surface` code whose framework the crate doesn't declare itself.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=IOSurface");
        // Location-independent install name: the app and every plugin record
        // `@rpath/libdelight_sdk.dylib`, so a plugin built elsewhere binds to
        // the copy the app already loaded instead of its own build's.
        println!("cargo:rustc-link-arg=-Wl,-install_name,@rpath/libdelight_sdk.dylib");
    }
}

/// FNV-1a over every file's relative path and contents, in path order — stable
/// across machines and checkout locations.
fn source_hash(root: &Path) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    let files = walkdir::WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file());
    for entry in files {
        let path = entry.path();
        feed(path.to_string_lossy().replace('\\', "/").as_bytes());
        feed(&[0]);
        feed(&std::fs::read(path).unwrap_or_default());
        feed(&[0]);
    }
    hash
}
