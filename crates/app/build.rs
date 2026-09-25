// Where the app finds libdelight_sdk.dylib and Rust's libstd at runtime:
// Contents/Frameworks in the app bundle (see scripts/bundle.sh), or next to
// the binary in a dev build. (`cargo run` itself adds the toolchain's libstd.)
fn main() {
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path/../Frameworks");
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path");
}
