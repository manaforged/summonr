use std::path::{Component, Path, PathBuf};

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
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && (name.starts_with(".env.")
            || name.starts_with("credentials.")
            || SECRET_FILES.contains(&name))
    {
        return true;
    }
    protected_dirs().any(|dir| path.starts_with(dir))
}

pub(crate) fn holds_protected(path: &Path) -> bool {
    protected_dirs().any(|dir| dir.starts_with(path))
}

fn protected_dirs() -> impl Iterator<Item = PathBuf> {
    let home = crate::config::home_dir();
    SECRET_DIRS
        .iter()
        .filter_map(move |dir| home.as_ref().map(|home| home.join(dir)))
        .chain(crate::prompt::summonr_home())
        .flat_map(|dir| {
            let real = dir.canonicalize().ok().filter(|real| *real != dir);
            std::iter::once(dir).chain(real)
        })
}
