//! Links the browser build with an 8 MiB stack so solidcore algorithms do not overflow the WASM stack.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32") {
        println!("cargo:rustc-link-arg=-zstack-size=8388608");
    }
}
