use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

const WORKSPACE_CONTROL: &[&[&str]] = &[&[".summonr", "check.toml"], &[".summonr", "policy.toml"]];

pub(crate) fn workspace_control(path: &Path) -> bool {
    let parts: Vec<&str> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => name.to_str(),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => None,
        })
        .collect();
    WORKSPACE_CONTROL.iter().any(|tail| parts.ends_with(tail))
}

const SECRET_FILES: &[&str] = &[
    ".env",
    "credentials",
    ".netrc",
    ".git-credentials",
    ".pypirc",
];
const SECRET_DIRS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".summonr",
    ".docker",
    ".kube",
    ".config/gh",
    ".config/gcloud",
];

pub(crate) fn protected_read(path: &Path) -> bool {
    let path = dunce::simplified(path);
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && (name.starts_with(".env.")
            || name.starts_with("credentials.")
            || SECRET_FILES.contains(&name))
    {
        return true;
    }
    protected_dirs().iter().any(|dir| within(path, dir))
}

pub(crate) fn holds_protected(path: &Path) -> bool {
    protected_dirs().iter().any(|dir| within(dir, path))
}

pub(crate) fn home_secret(path: &Path, dirs: &[&str]) -> bool {
    crate::config::home_dir().is_some_and(|home| secret_under(path, &home, dirs))
        || crate::prompt::summonr_home()
            .ok()
            .is_some_and(|summonr| within(path, &summonr))
}

pub(crate) fn secret_under(path: &Path, home: &Path, dirs: &[&str]) -> bool {
    let real = home.canonicalize().ok().filter(|real| real != home);
    let homes: Vec<PathBuf> = std::iter::once(home.to_path_buf()).chain(real).collect();
    let guarded: Vec<PathBuf> = homes
        .iter()
        .flat_map(|home| dirs.iter().map(move |dir| home.join(dir)))
        .collect();
    guarded.iter().any(|dir| within(path, dir))
}

fn within(path: &Path, dir: &Path) -> bool {
    folded(path).starts_with(folded(dir))
}

fn folded(path: &Path) -> PathBuf {
    let plain = dunce::simplified(path);
    if cfg!(windows) {
        PathBuf::from(plain.to_string_lossy().to_lowercase())
    } else {
        plain.to_path_buf()
    }
}

type Dirs = (Option<PathBuf>, Option<PathBuf>, Arc<Vec<PathBuf>>);

fn protected_dirs() -> Arc<Vec<PathBuf>> {
    static CACHE: Mutex<Option<Dirs>> = Mutex::new(None);
    let home = crate::config::home_dir();
    let summonr = crate::prompt::summonr_home().ok();
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((cached_home, cached_summonr, dirs)) = cache.as_ref()
        && *cached_home == home
        && *cached_summonr == summonr
    {
        return Arc::clone(dirs);
    }
    let dirs: Arc<Vec<PathBuf>> = Arc::new(
        SECRET_DIRS
            .iter()
            .filter_map(|dir| home.as_ref().map(|home| home.join(dir)))
            .chain(summonr.clone())
            .flat_map(|dir| {
                let real = dir.canonicalize().ok().filter(|real| *real != dir);
                std::iter::once(dir).chain(real)
            })
            .collect(),
    );
    *cache = Some((home, summonr, Arc::clone(&dirs)));
    dirs
}

#[cfg(test)]
#[path = "protected_tests.rs"]
mod tests;
