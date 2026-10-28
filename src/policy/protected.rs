use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use super::rules;

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
    rules().secrets.control.iter().any(|tail| {
        parts.len() >= tail.len()
            && parts[parts.len() - tail.len()..]
                .iter()
                .zip(tail)
                .all(|(part, name)| *part == name.as_str())
    })
}

pub(crate) fn protected_read(path: &Path) -> bool {
    let path = dunce::simplified(path);
    let secrets = &rules().secrets;
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && secret_name(name, &secrets.read_files, &secrets.read_prefixes)
    {
        return true;
    }
    protected_dirs().iter().any(|dir| within(path, dir))
}

pub(super) fn secret_name(name: &str, files: &[String], prefixes: &[String]) -> bool {
    files.iter().any(|file| file == name)
        || prefixes
            .iter()
            .any(|prefix| name.starts_with(prefix.as_str()))
}

pub(crate) fn holds_protected(path: &Path) -> bool {
    protected_dirs().iter().any(|dir| within(dir, path))
}

pub(crate) fn home_secret<S: AsRef<str>>(path: &Path, dirs: &[S]) -> bool {
    summonr_core::dirs::home_dir().is_some_and(|home| secret_under(path, &home, dirs))
        || summonr_core::dirs::summonr_home()
            .ok()
            .is_some_and(|summonr| within(path, &summonr))
}

pub(crate) fn secret_under<S: AsRef<str>>(path: &Path, home: &Path, dirs: &[S]) -> bool {
    let real = home.canonicalize().ok().filter(|real| real != home);
    let homes: Vec<PathBuf> = std::iter::once(home.to_path_buf()).chain(real).collect();
    let guarded: Vec<PathBuf> = homes
        .iter()
        .flat_map(|home| dirs.iter().map(move |dir| home.join(dir.as_ref())))
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
    let home = summonr_core::dirs::home_dir();
    let summonr = summonr_core::dirs::summonr_home().ok();
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((cached_home, cached_summonr, dirs)) = cache.as_ref()
        && *cached_home == home
        && *cached_summonr == summonr
    {
        return Arc::clone(dirs);
    }
    let dirs: Arc<Vec<PathBuf>> = Arc::new(
        rules()
            .secrets
            .read_dirs
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
