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
    protected_dirs().iter().any(|dir| path.starts_with(dir))
}

pub(crate) fn holds_protected(path: &Path) -> bool {
    let path = dunce::simplified(path);
    protected_dirs().iter().any(|dir| dir.starts_with(path))
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
