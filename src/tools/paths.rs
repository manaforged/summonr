use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

use super::{ToolRegistry, sandbox};

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

pub(super) struct Root {
    path: PathBuf,
    canonical: Option<PathBuf>,
}

impl Root {
    fn resolved(&self) -> Cow<'_, Path> {
        match &self.canonical {
            Some(canonical) => Cow::Borrowed(canonical.as_path()),
            None => Cow::Owned(
                self.path
                    .canonicalize()
                    .unwrap_or_else(|_| self.path.clone()),
            ),
        }
    }
}

pub(super) fn roots(policy: &crate::policy::Policy) -> Vec<Root> {
    policy
        .write_roots()
        .into_iter()
        .map(|path| Root {
            canonical: path.canonicalize().ok(),
            path,
        })
        .collect()
}

impl ToolRegistry {
    pub(crate) fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.lexical(path)?.exists())
    }

    pub(crate) fn enable_sandbox(&mut self, network: sandbox::Network) -> Result<()> {
        self.sandbox = Some(sandbox::Sandbox::discover(
            &self.workspace,
            network,
            &self.policy.write_roots(),
        )?);
        Ok(())
    }

    pub(crate) fn disable_sandbox(&mut self) {
        self.sandbox = None;
    }

    pub(crate) fn allow_path(&mut self, path: PathBuf) -> Result<()> {
        if !path.exists() {
            if volume_unmounted(&path) {
                return Err(Error::Config(format!(
                    "write path {}: volume is not mounted",
                    path.display()
                )));
            }
            std::fs::create_dir_all(&path).map_err(|error| {
                Error::Config(format!(
                    "cannot create write path {}: {error}",
                    path.display()
                ))
            })?;
        }
        let resolved = path.canonicalize().map_err(|error| {
            Error::Config(format!(
                "write path {} is unusable: {error}",
                path.display()
            ))
        })?;
        if !resolved.is_dir() {
            return Err(Error::Config(format!(
                "write path {} is not a directory",
                resolved.display()
            )));
        }
        self.policy.allow_path(resolved);
        self.roots = roots(&self.policy);
        if self.sandbox.is_some() {
            let network = sandbox::Network::allowed(!self.denies_network());
            self.enable_sandbox(network)?;
        }
        Ok(())
    }

    pub(crate) fn write_roots(&self) -> Vec<String> {
        self.policy
            .write_roots()
            .iter()
            .map(|path| path.display().to_string())
            .collect()
    }

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
        if !self.policy.is_unrestricted() {
            if crate::policy::protected_read(&opened) {
                return Err(Error::Denied(format!("`{path}` is a protected path")));
            }
            if !self.inside_fence(&opened) {
                return Err(Error::Denied(format!("`{path}` is outside the workspace")));
            }
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
        if self.roots.is_empty() {
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
        self.roots.iter().any(|root| {
            let root = root.resolved();
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
        crate::config::home_dir()
            .map(|home| {
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
