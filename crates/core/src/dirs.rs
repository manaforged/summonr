use std::path::PathBuf;

use crate::error::{Error, Result};

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

static READY: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

pub fn summonr_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SUMMONR_HOME") {
        if path.is_empty() {
            return Err(Error::Config("SUMMONR_HOME is empty".to_string()));
        }
        return Ok(PathBuf::from(path));
    }
    let home = user_home().ok_or_else(|| {
        Error::Config("set SUMMONR_HOME because the home directory is unknown".to_string())
    })?;
    let dir = home.join(".summonr");
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

#[cfg(not(any(test, feature = "test-home")))]
pub fn user_home() -> Option<PathBuf> {
    home_dir()
}

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
