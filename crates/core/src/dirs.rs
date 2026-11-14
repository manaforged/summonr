use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The name of the summonr state folder, in the home directory and in a workspace.
pub const STATE: &str = ".summonr";

/// The environment variable that moves the summonr home folder.
pub const HOME_ENV: &str = "SUMMONR_HOME";

/// The environment variable that lists the folders a process searches for programs.
pub const PATH_ENV: &str = "PATH";

/// The `.summonr` folder inside a workspace.
#[must_use]
pub fn workspace(root: &Path) -> PathBuf {
    root.join(STATE)
}

/// The name of Claude Code's config folder, in the home directory and in a project.
pub const CLAUDE: &str = ".claude";

/// The `.claude` folder inside `root`, a home directory or a project.
#[must_use]
pub fn claude(root: &Path) -> PathBuf {
    root.join(CLAUDE)
}

/// The user's home directory from `HOME`, then `USERPROFILE`.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

static READY: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// The summonr home folder: `SUMMONR_HOME`, or `~/.summonr` created with mode 0700.
pub fn summonr_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(HOME_ENV) {
        if path.is_empty() {
            return Err(Error::Config("SUMMONR_HOME is empty".to_string()));
        }
        return Ok(PathBuf::from(path));
    }
    let home = user_home().ok_or_else(|| {
        Error::Config("set SUMMONR_HOME because the home directory is unknown".to_string())
    })?;
    let dir = home.join(STATE);
    if READY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        == Some(&dir)
    {
        return Ok(dir);
    }
    if !dir.is_dir() {
        std::fs::create_dir_all(&dir)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&dir)?.permissions().mode() & 0o777;
        if mode != 0o700 {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    *READY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(dir.clone());
    Ok(dir)
}

/// The home directory that holds `~/.summonr`.
#[cfg(not(any(test, feature = "test-home")))]
pub fn user_home() -> Option<PathBuf> {
    home_dir()
}

/// A per-process temporary home directory for tests.
#[cfg(any(test, feature = "test-home"))]
pub fn user_home() -> Option<PathBuf> {
    static HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    Some(
        HOME.get_or_init(|| {
            std::env::temp_dir().join(format!("summonr-tests-{}", std::process::id()))
        })
        .clone(),
    )
}

/// The test fixture `name` under the repository's `tests/fixtures`.
#[cfg(any(test, feature = "test-home"))]
#[must_use]
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// Symlinks the test fixture `name` at `at` and returns `at`.
#[cfg(all(unix, any(test, feature = "test-home")))]
pub fn link_fixture(name: &str, at: &Path) -> PathBuf {
    std::os::unix::fs::symlink(fixture(name), at).unwrap();
    at.to_path_buf()
}
