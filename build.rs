fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!(
        "cargo:rustc-env=SUMMONR_TARGET={}",
        std::env::var("TARGET").unwrap_or_default()
    );
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "windows" {
        let flag = match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
            Ok("msvc") => format!("/STACK:{MAIN_STACK}"),
            _ => format!("-Wl,--stack,{MAIN_STACK}"),
        };
        println!("cargo:rustc-link-arg-bins={flag}");
    }
}

const MAIN_STACK: u32 = 8 * 1024 * 1024;
