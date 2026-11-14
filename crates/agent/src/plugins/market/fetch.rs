use std::path::{Path, PathBuf};

use crate::process::Deadline;
use summonr_core::error::{Error, Result};

use super::catalog::{Remote, dir_under};
use super::origin::{REPO, github_url, repo_ok, ssh};

const SCHEMES: [&str; 4] = ["https://", "http://", "file://", "ssh://"];
const SHA_LEN: usize = 40;
const PLUGIN: &str = "plugin";

/// A branch or tag, and a commit that wins over it.
pub(super) struct Pin<'a> {
    reference: Option<&'a str>,
    sha: Option<&'a str>,
}

impl<'a> Pin<'a> {
    pub(super) fn new(reference: Option<&'a str>, sha: Option<&'a str>) -> Self {
        Self { reference, sha }
    }

    fn check(&self) -> Result<()> {
        if let Some(reference) = self.reference
            && !word(reference)
        {
            return Err(Error::Config(format!(
                "ref `{reference}` is not a branch or tag"
            )));
        }
        if let Some(sha) = self.sha
            && !(sha.len() == SHA_LEN && sha.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')))
        {
            return Err(Error::Config(format!(
                "sha `{sha}` is not a full 40-character lowercase commit SHA"
            )));
        }
        Ok(())
    }
}

impl Remote {
    /// Fetches this source under `work` and returns the plugin directory; nothing it holds runs.
    pub(super) fn fetch(&self, plugin: &str, work: &Path) -> Result<PathBuf> {
        let dir = work.join(PLUGIN);
        match self {
            Self::Github {
                repo,
                reference,
                sha,
            } => {
                if !repo_ok(repo) {
                    return Err(Error::Config(format!(
                        "plugin `{plugin}`: `{repo}` is not a GitHub owner/repo"
                    )));
                }
                clone(
                    &github_url(repo),
                    &Pin::new(reference.as_deref(), sha.as_deref()),
                    None,
                    &dir,
                )?;
            }
            Self::Url {
                url,
                reference,
                sha,
            } => clone(
                url,
                &Pin::new(reference.as_deref(), sha.as_deref()),
                None,
                &dir,
            )?,
            Self::GitSubdir {
                url,
                path,
                reference,
                sha,
            } => {
                let url = if repo_ok(url) {
                    github_url(url)
                } else {
                    url.clone()
                };
                let repo = work.join(REPO);
                let refused = || {
                    Error::Config(format!(
                        "plugin `{plugin}`: path `{path}` leaves the repository"
                    ))
                };
                let inner = path.trim_start_matches("./").trim_end_matches('/');
                if inner.is_empty() || !word(inner) {
                    return Err(refused());
                }
                let at = super::super::inside(&repo, inner).ok_or_else(refused)?;
                clone(
                    &url,
                    &Pin::new(reference.as_deref(), sha.as_deref()),
                    Some(inner),
                    &repo,
                )?;
                return dir_under(&at, &repo)?.ok_or_else(refused);
            }
            Self::Npm {} => return Err(unfetched(plugin, "npm")),
            Self::Archive {} => return Err(unfetched(plugin, "archive")),
            Self::Command {} => return Err(unfetched(plugin, "command")),
        }
        Ok(dir)
    }
}

fn unfetched(plugin: &str, kind: &str) -> Error {
    Error::Config(format!(
        "plugin `{plugin}` has a `{kind}` source, which summonr does not fetch; it fetches relative, github, url, and git-subdir sources"
    ))
}

/// Clones `url` shallow into `dest`, at `pin`, and only `subdir` when given.
pub(super) fn clone(url: &str, pin: &Pin<'_>, subdir: Option<&str>, dest: &Path) -> Result<()> {
    if !word(url) || !(ssh(url) || SCHEMES.iter().any(|scheme| url.starts_with(scheme))) {
        return Err(Error::Config(format!(
            "`{url}` is not a git URL: use https://, http://, file://, ssh://, or user@host:path"
        )));
    }
    pin.check()?;
    let parent = dest
        .parent()
        .ok_or_else(|| Error::Config(format!("{} has no parent", dest.display())))?;
    std::fs::create_dir_all(parent)?;
    let target = dest.to_string_lossy().into_owned();
    let mut args = vec!["clone", "--quiet", "--depth", "1"];
    if pin.sha.is_some() {
        args.push("--no-checkout");
    }
    if subdir.is_some() {
        args.extend(["--filter=blob:none", "--sparse"]);
    }
    if let (Some(reference), None) = (pin.reference, pin.sha) {
        args.extend(["--branch", reference]);
    }
    args.extend(["--", url, target.as_str()]);
    git(parent, &args)?;
    if let Some(path) = subdir {
        git(dest, &["sparse-checkout", "set", path])?;
    }
    if let Some(sha) = pin.sha {
        git(dest, &["fetch", "--quiet", "--depth", "1", "origin", sha])?;
        git(dest, &["checkout", "--quiet", "--detach", sha])?;
    }
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<()> {
    let limits = crate::limits::limits();
    let deadline = Deadline::new(limits.plugin_fetch(), limits.plugin_fetch_poll());
    crate::diff::git_within(root, args, deadline).map(drop)
}

/// Downloads `url` within `plugin_fetch_secs`, refusing a body over `web_body_bytes`.
pub(super) async fn download(url: &str) -> Result<Vec<u8>> {
    let limits = crate::limits::limits();
    let http = crate::credentials::http(limits.plugin_fetch())?;
    crate::credentials::download(&http, url, Some(limits.web_body_bytes))
        .await
        .map_err(Error::Config)
}

/// Refuses a link under `dir` that names a directory or leaves `dir`, so a copy stays finite and local.
pub(super) fn contained(dir: &Path) -> Result<()> {
    let base = dir.canonicalize()?;
    let mut pending = vec![base.clone()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                let target = path.canonicalize().ok();
                if !target.is_some_and(|target| target.starts_with(&base) && target.is_file()) {
                    return Err(Error::Config(format!(
                        "{} is a link that leaves the plugin or names a directory; not installed",
                        path.display()
                    )));
                }
            } else if kind.is_dir() && entry.file_name() != ".git" {
                pending.push(path);
            }
        }
    }
    Ok(())
}

fn word(text: &str) -> bool {
    !text.is_empty() && !text.starts_with('-') && text.chars().all(|c| c.is_ascii_graphic())
}
