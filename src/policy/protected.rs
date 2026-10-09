use std::path::{Component, Path};

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
