use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo::rustc-check-cfg=cfg(sandboxed)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let windows = std::env::var_os("CARGO_FEATURE_WINDOWS_SANDBOX").is_some();
    if os == "macos" || os == "linux" || (os == "windows" && windows) {
        println!("cargo:rustc-cfg=sandboxed");
    }
    if let Ok(head) = std::fs::read_to_string(".git/HEAD")
        && let Some(reference) = head.strip_prefix("ref: ").map(str::trim)
    {
        println!("cargo:rerun-if-changed=.git/{reference}");
    }
    let Ok(output) = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let mut revision = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let dirty = Command::new("git")
        .args(["diff", "--quiet", "HEAD", "--"])
        .status()
        .is_ok_and(|status| !status.success());
    if dirty {
        revision.push_str("-dirty");
    }
    if !revision.is_empty() {
        println!("cargo:rustc-env=SUMMONR_REVISION={revision}");
    }
}
