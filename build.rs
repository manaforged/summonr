use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo::rustc-check-cfg=cfg(sandboxed)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let windows = std::env::var_os("CARGO_FEATURE_WINDOWS_SANDBOX").is_some();
    if os == "macos" || os == "linux" || (os == "windows" && windows) {
        println!("cargo:rustc-cfg=sandboxed");
    }
    if let Some(head) = git_path("HEAD") {
        println!("cargo:rerun-if-changed={head}");
        if let Some(reference) = std::fs::read_to_string(&head)
            .ok()
            .and_then(|head| {
                head.strip_prefix("ref: ")
                    .map(|name| name.trim().to_string())
            })
            .and_then(|name| git_path(&name))
        {
            println!("cargo:rerun-if-changed={reference}");
        }
    }
    println!("cargo:rerun-if-env-changed={REVISION_ENV}");
    println!("cargo:rerun-if-env-changed={CI_SHA_ENV}");
    if let Some(revision) = revision() {
        println!("cargo:rustc-env={REVISION_ENV}={revision}");
    }
}

const REVISION_ENV: &str = "SUMMONR_REVISION";
const CI_SHA_ENV: &str = "GITHUB_SHA";
const SHORT_LEN: usize = 12;

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn revision() -> Option<String> {
    env_value(REVISION_ENV)
        .or_else(|| env_value(CI_SHA_ENV).map(|sha| sha.chars().take(SHORT_LEN).collect()))
        .or_else(git_revision)
}

fn git_revision() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", &format!("--short={SHORT_LEN}"), "HEAD"])
        .output()
        .ok()?;
    let mut revision = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || revision.is_empty() {
        return None;
    }
    let dirty = Command::new("git")
        .args(["diff", "--quiet", "HEAD", "--"])
        .status()
        .is_ok_and(|status| !status.success());
    if dirty {
        revision.push_str("-dirty");
    }
    Some(revision)
}

fn git_path(name: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-path", name])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && std::path::Path::new(&path).exists()).then_some(path)
}
