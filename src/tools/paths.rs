use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

use super::ToolRegistry;

pub(super) fn volume_unmounted(path: &Path) -> bool {
    let mut parts = path.components();
    if parts.next() != Some(Component::RootDir) {
        return false;
    }
    match parts.next() {
        Some(Component::Normal(volumes)) if volumes == "Volumes" => {}
        _ => return false,
    }
    let Some(Component::Normal(name)) = parts.next() else {
        return false;
    };
    !Path::new("/Volumes").join(name).is_dir()
}

impl ToolRegistry {
    pub(in crate::tools) fn resolve_read(&self, path: &str) -> Result<PathBuf> {
        let cleaned = clean_path(path);
        let candidate = Path::new(cleaned.as_ref());
        let opened = if candidate.is_absolute() {
            existing(candidate)
                .canonicalize()
                .map_err(|error| Error::Tool(format!("cannot open `{path}`: {error}")))?
        } else {
            self.resolve_existing(path)?
        };
        if !self.policy.is_unrestricted() && crate::policy::protected_read(&opened) {
            return Err(Error::Denied(format!("`{path}` is a protected path")));
        }
        Ok(opened)
    }

    pub(in crate::tools) fn resolve_existing(&self, path: &str) -> Result<PathBuf> {
        let candidate = self.lexical(path)?;
        let canonical = existing(&candidate)
            .canonicalize()
            .map_err(|error| Error::Tool(format!("cannot open `{path}`: {error}")))?;
        self.ensure_inside(&canonical)?;
        Ok(canonical)
    }

    pub(in crate::tools) fn resolve_write(&self, path: &str) -> Result<PathBuf> {
        let dest = self.lexical(path)?;
        if self.policy.is_unrestricted() {
            return Ok(dest);
        }
        if dest.exists() {
            return self.resolve_existing(path);
        }
        let mut ancestor = dest.parent().unwrap_or(&self.workspace).to_path_buf();
        while !ancestor.exists() {
            match ancestor.parent() {
                Some(parent) => ancestor = parent.to_path_buf(),
                None => break,
            }
        }
        if ancestor.exists() {
            let canonical = ancestor
                .canonicalize()
                .map_err(|error| Error::Tool(format!("cannot create `{path}`: {error}")))?;
            self.ensure_inside(&canonical)?;
        } else if !self.inside_fence(&ancestor) {
            return Err(Error::Tool(crate::policy::write_denied(
                &ancestor.display().to_string(),
            )));
        }
        Ok(dest)
    }

    pub(in crate::tools) fn lexical(&self, path: &str) -> Result<PathBuf> {
        let cleaned = clean_path(path);
        let relative = Path::new(cleaned.as_ref());
        if path.trim().is_empty() {
            return Err(Error::Tool("path is empty".to_string()));
        }
        if self.policy.is_unrestricted() {
            return Ok(if relative.is_absolute() {
                relative.to_path_buf()
            } else {
                self.workspace.join(relative)
            });
        }
        if relative.is_absolute() {
            let path = crate::policy::expand_path(path);
            self.ensure_inside(&path)?;
            return Ok(path);
        }
        let mut out = self.workspace.clone();
        for component in relative.components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        return Err(Error::Tool(crate::policy::write_denied(
                            &out.display().to_string(),
                        )));
                    }
                }
                Component::Prefix(_) | Component::RootDir => {
                    return Err(Error::Tool(
                        "tools accept workspace-relative paths".to_string(),
                    ));
                }
            }
        }
        self.ensure_inside(&out)?;
        Ok(out)
    }

    pub(in crate::tools) fn inside_fence(&self, path: &Path) -> bool {
        self.policy.is_unrestricted() || path.starts_with(&self.workspace) || self.approved(path)
    }

    pub(in crate::tools) fn approved(&self, path: &Path) -> bool {
        let roots = self.policy.write_roots();
        if roots.is_empty() {
            return false;
        }
        let resolved = path.canonicalize().unwrap_or_else(|_| {
            path.parent()
                .and_then(|parent| parent.canonicalize().ok())
                .map_or_else(
                    || path.to_path_buf(),
                    |parent| parent.join(path.file_name().unwrap_or_default()),
                )
        });
        roots.iter().any(|root| {
            let root = root.canonicalize().unwrap_or_else(|_| root.clone());
            resolved.starts_with(&root) || path.starts_with(&root)
        })
    }

    pub(in crate::tools) fn check_write(&self, path: &Path) -> Result<()> {
        if self.policy.is_unrestricted() {
            return Ok(());
        }
        if crate::policy::protected_write(path) {
            return Err(Error::Denied(format!(
                "`{}` is a protected path",
                path.display()
            )));
        }
        if self.approved(path) {
            return Ok(());
        }
        self.policy.write_path(&self.relative(path)?)
    }

    pub(in crate::tools) fn ensure_inside(&self, path: &Path) -> Result<()> {
        if self.inside_fence(path) {
            Ok(())
        } else {
            Err(Error::Tool(crate::policy::write_denied(
                &path.display().to_string(),
            )))
        }
    }

    pub(in crate::tools) fn relative(&self, path: &Path) -> Result<String> {
        if self.policy.is_unrestricted() {
            return Ok(path.to_string_lossy().into_owned());
        }
        path.strip_prefix(&self.workspace)
            .map(|relative| relative.to_string_lossy().into_owned())
            .map_err(|_error| Error::Tool(crate::policy::write_denied(&path.display().to_string())))
    }
}

fn existing(path: &Path) -> PathBuf {
    use unicode_normalization::UnicodeNormalization;
    if path.exists() {
        return path.to_path_buf();
    }
    let fold = |name: &std::ffi::OsStr| name.to_string_lossy().nfkc().collect::<String>();
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return path.to_path_buf();
    };
    let wanted = fold(name);
    let Ok(entries) = std::fs::read_dir(existing(parent)) else {
        return path.to_path_buf();
    };
    let mut found = entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| fold(&entry.file_name()) == wanted);
    match (found.next(), found.next()) {
        (Some(entry), None) => entry.path(),
        _ => path.to_path_buf(),
    }
}

fn clean_path(path: &str) -> Cow<'_, str> {
    let expanded = if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| {
                let home = PathBuf::from(home);
                if path == "~" {
                    home.to_string_lossy().into_owned()
                } else {
                    home.join(&path[2..]).to_string_lossy().into_owned()
                }
            })
            .unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    };
    if expanded == path {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(expanded)
    }
}
