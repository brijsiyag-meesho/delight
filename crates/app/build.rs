// Where the app finds libdelight_sdk.dylib and Rust's libstd at runtime:
// Contents/Frameworks in the bundle (see scripts/bundle.sh); next to the
// binary and in the toolchain for dev builds run outside `cargo run`.
fn main() {
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path/../Frameworks");
    if std::env::var("PROFILE").as_deref() == Ok("debug") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path");
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
        let sysroot = std::process::Command::new(rustc)
            .args(["--print", "sysroot"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok());
        if let (Some(sysroot), Ok(target)) = (sysroot, std::env::var("TARGET")) {
            println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{}/lib/rustlib/{target}/lib", sysroot.trim());
        }
    }
}
