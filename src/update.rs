use std::cmp::Ordering;
use std::path::Path;
use std::process::Command;

use leyline::Session;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

pub const RELEASES: &str = "https://github.com/manaforged/summonr/releases";
const FROM_SOURCE: &str = "cargo install --git https://github.com/manaforged/summonr --locked";
const BINARY: &str = if cfg!(windows) {
    "summonr.exe"
} else {
    "summonr"
};
const ARCHIVE: &str = if cfg!(windows) { "zip" } else { "tar.gz" };

pub struct Source<'a> {
    pub releases: &'a str,
    pub target: &'a str,
    pub current: &'a str,
}

impl Source<'static> {
    #[must_use]
    pub const fn this_build() -> Self {
        Self {
            releases: RELEASES,
            target: env!("SUMMONR_TARGET"),
            current: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Current(String),
    Ahead { latest: String },
    Updated { from: String, to: String },
}

pub async fn run(source: &Source<'_>, exe: &Path) -> Result<Outcome> {
    if built_from_source(exe) {
        return Err(Error::Setup(format!(
            "{} was built from source; update it with `{FROM_SOURCE}`",
            exe.display()
        )));
    }
    let http = crate::credentials::session(
        summonr_auth::builder(
            crate::limits::limits().update(),
            crate::limits::limits().connect(),
        ),
        crate::limits::limits().retry_policy(crate::limits::Reach::Remote),
    )?;
    let latest = latest(&http, source.releases).await?;
    match newer(&latest, source.current) {
        Ordering::Equal => return Ok(Outcome::Current(latest)),
        Ordering::Less => return Ok(Outcome::Ahead { latest }),
        Ordering::Greater => {}
    }
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let stage = tempfile::Builder::new()
        .prefix(".summonr-update-")
        .tempdir_in(dir)
        .map_err(|error| unwritable(dir, &error))?;
    let name = format!("summonr-{}", source.target);
    let asset = format!("{name}.{ARCHIVE}");
    let base = format!("{}/download/v{latest}", source.releases);
    eprintln!("Downloading {asset} ({latest})");
    let archive = fetch(&http, &format!("{base}/{asset}")).await?;
    let listed = fetch(&http, &format!("{base}/{asset}.sha256")).await?;
    check(&archive, &listed, &asset)?;
    unpack(stage.path(), &asset, &archive)?;
    let fresh = stage.path().join(&name).join(BINARY);
    runs_as(&fresh, &latest)?;
    swap(exe, &fresh)?;
    Ok(Outcome::Updated {
        from: source.current.to_string(),
        to: latest,
    })
}

fn built_from_source(exe: &Path) -> bool {
    let Some(dir) = exe.parent() else {
        return false;
    };
    dir.join(".fingerprint").exists()
        || dir.parent().is_some_and(|home| {
            home.join(".crates2.json").exists() || home.join(".crates.toml").exists()
        })
}

async fn latest(http: &Session, releases: &str) -> Result<String> {
    let none = || Error::Setup(format!("no release is published at {releases}"));
    let response = http
        .get(format!("{releases}/latest"))
        .send()
        .await
        .map_err(|error| Error::Setup(format!("cannot reach {releases}: {error}")))?;
    if !response.status().is_success() {
        return Err(none());
    }
    response
        .url()
        .path_segments()
        .and_then(Iterator::last)
        .and_then(|tag| tag.strip_prefix('v'))
        .filter(|version| core(version).is_some())
        .map(str::to_string)
        .ok_or_else(none)
}

fn core(version: &str) -> Option<[u64; 3]> {
    let parts: Vec<u64> = version
        .split(['-', '+'])
        .next()?
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    parts.try_into().ok()
}

fn newer(latest: &str, current: &str) -> Ordering {
    core(latest)
        .cmp(&core(current))
        .then_with(|| current.contains('-').cmp(&latest.contains('-')))
}

async fn fetch(http: &Session, url: &str) -> Result<Vec<u8>> {
    let failed = |why: String| Error::Setup(format!("cannot download {url}: {why}"));
    let response = http
        .get(url)
        .send()
        .await
        .map_err(|error| failed(error.to_string()))?;
    if !response.status().is_success() {
        return Err(failed(format!("HTTP {}", response.status().as_u16())));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| failed(error.to_string()))
}

fn check(archive: &[u8], listed: &[u8], asset: &str) -> Result<()> {
    let want = String::from_utf8_lossy(listed)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let got = format!("{:x}", Sha256::digest(archive));
    if want == got {
        return Ok(());
    }
    Err(Error::Setup(format!(
        "checksum mismatch for {asset}: expected {want}, got {got}; nothing was replaced"
    )))
}

fn unpack(stage: &Path, asset: &str, archive: &[u8]) -> Result<()> {
    let file = stage.join(asset);
    std::fs::write(&file, archive)?;
    let status = Command::new("tar")
        .arg("-xf")
        .arg(&file)
        .arg("-C")
        .arg(stage)
        .status()
        .map_err(|error| Error::Setup(format!("cannot run tar to unpack {asset}: {error}")))?;
    if !status.success() {
        return Err(Error::Setup(format!("tar could not unpack {asset}")));
    }
    Ok(())
}

fn runs_as(fresh: &Path, version: &str) -> Result<()> {
    let output = Command::new(fresh).arg("--version").output();
    let printed = output
        .as_ref()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();
    if output.is_ok_and(|output| output.status.success())
        && printed.split_whitespace().any(|word| word == version)
    {
        return Ok(());
    }
    Err(Error::Setup(format!(
        "the downloaded summonr {version} did not start (printed `{printed}`); nothing was replaced"
    )))
}

#[cfg(unix)]
fn swap(exe: &Path, fresh: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(fresh, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(fresh, exe).map_err(|error| unwritable(exe, &error))
}

#[cfg(windows)]
fn swap(exe: &Path, fresh: &Path) -> Result<()> {
    let old = exe.with_extension("exe.old");
    drop(std::fs::remove_file(&old));
    std::fs::rename(exe, &old).map_err(|error| unwritable(exe, &error))?;
    std::fs::rename(fresh, exe).map_err(|error| {
        drop(std::fs::rename(&old, exe));
        unwritable(exe, &error)
    })
}

fn unwritable(path: &Path, error: &std::io::Error) -> Error {
    Error::Setup(format!(
        "cannot replace {}: {error}; run `summonr update` with rights to write there",
        path.display()
    ))
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
