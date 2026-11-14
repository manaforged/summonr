use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use summonr_core::error::{Error, Result};

use super::catalog::CATALOG;
use super::fetch::{self, Pin};

const GITHUB: &str = "https://github.com/";
const GIT_HOSTS: [&str; 2] = ["github.com", "gitlab.com"];
const OFFICIAL_OWNER: &str = "anthropics/";
const OFFICIAL_GIT: [&str; 4] = [
    "https://github.com/",
    "http://github.com/",
    "git@github.com:",
    "ssh://git@github.com/",
];
const SHAPES: &str = "Invalid marketplace source format. Try: owner/repo, https://..., or ./path";
pub(super) const REPO: &str = "repo";

/// Where a marketplace's `marketplace.json` comes from, as `marketplace add` reads its argument.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub(super) enum Origin {
    Github {
        repo: String,
        #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
    },
    Git {
        url: String,
        #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
    },
    Url {
        url: String,
    },
    Directory {
        path: PathBuf,
    },
    File {
        path: PathBuf,
    },
}

impl Origin {
    pub(super) fn parse(input: &str) -> Result<Self> {
        if local(input) {
            return in_place(input);
        }
        let (body, reference) = match input.rsplit_once('#') {
            Some((body, reference)) => (body, Some(reference.to_string())),
            None => (input, None),
        };
        if body
            .split(['/', ':'])
            .any(|part| matches!(part, "." | ".."))
        {
            return Err(Error::Config(format!(
                "{SHAPES}; `{input}` has a `.` or `..` path segment"
            )));
        }
        if input.starts_with("http://") || input.starts_with("https://") {
            return Ok(web(input, body, reference));
        }
        if ssh(body) {
            return Ok(Self::Git {
                url: body.to_string(),
                reference,
            });
        }
        let (repo, reference) = match input.split_once(['#', '@']) {
            Some((repo, reference)) => (repo, Some(reference.to_string())),
            None => (input, None),
        };
        if !repo_ok(repo) {
            return Err(Error::Config(format!(
                "{SHAPES}; a host name needs `https://` in front"
            )));
        }
        Ok(Self::Github {
            repo: repo.to_string(),
            reference,
        })
    }

    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Github { .. } => "github",
            Self::Git { .. } => "git",
            Self::Url { .. } => "url",
            Self::Directory { .. } => "directory",
            Self::File { .. } => "file",
        }
    }

    /// The marketplace root: where relative plugin sources resolve.
    pub(super) fn root(&self, dir: &Path) -> PathBuf {
        match self {
            Self::Directory { path } => path.clone(),
            Self::File { path } => path.ancestors().nth(2).unwrap_or(path).to_path_buf(),
            Self::Github { .. } | Self::Git { .. } | Self::Url { .. } => dir.join(REPO),
        }
    }

    pub(super) fn catalog(&self, root: &Path) -> PathBuf {
        match self {
            Self::File { path } => path.clone(),
            Self::Github { .. } | Self::Git { .. } | Self::Url { .. } | Self::Directory { .. } => {
                root.join(CATALOG)
            }
        }
    }

    /// Whether summonr keeps its own copy, which `update` fetches again.
    pub(super) fn fetched(&self) -> bool {
        matches!(
            self,
            Self::Github { .. } | Self::Git { .. } | Self::Url { .. }
        )
    }

    /// Whether the plugin files are on disk, so relative plugin sources resolve.
    pub(super) fn has_files(&self) -> bool {
        !matches!(self, Self::Url { .. })
    }

    pub(super) fn official(&self) -> bool {
        let repo = match self {
            Self::Github { repo, .. } => Some(repo.as_str()),
            Self::Git { url, .. } => OFFICIAL_GIT
                .iter()
                .find_map(|prefix| url.strip_prefix(prefix))
                .map(|rest| rest.strip_suffix(".git").unwrap_or(rest)),
            Self::Url { .. } | Self::Directory { .. } | Self::File { .. } => None,
        };
        repo.is_some_and(|repo| repo_ok(repo) && repo.starts_with(OFFICIAL_OWNER))
    }

    pub(super) async fn fetch(&self, into: &Path) -> Result<()> {
        match self {
            Self::Github { repo, reference } => fetch::clone(
                &github_url(repo),
                &Pin::new(reference.as_deref(), None),
                None,
                into,
            ),
            Self::Git { url, reference } => {
                fetch::clone(url, &Pin::new(reference.as_deref(), None), None, into)
            }
            Self::Url { url } => {
                let bytes = fetch::download(url).await?;
                let path = into.join(CATALOG);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, bytes)?;
                Ok(())
            }
            Self::Directory { .. } | Self::File { .. } => Ok(()),
        }
    }

    pub(super) fn reference(&self) -> Option<&str> {
        match self {
            Self::Github { reference, .. } | Self::Git { reference, .. } => reference.as_deref(),
            Self::Url { .. } | Self::Directory { .. } | Self::File { .. } => None,
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let place = match self {
            Self::Github { repo, .. } => repo.clone(),
            Self::Git { url, .. } | Self::Url { url } => url.clone(),
            Self::Directory { path } | Self::File { path } => path.display().to_string(),
        };
        match self.reference() {
            Some(reference) => write!(formatter, "{place}#{reference}"),
            None => write!(formatter, "{place}"),
        }
    }
}

/// The clone URL of a GitHub `owner/repo`.
pub(super) fn github_url(repo: &str) -> String {
    format!("{GITHUB}{repo}.git")
}

/// Whether `repo` is a GitHub `owner/repo` under GitHub's naming rules.
pub(super) fn repo_ok(repo: &str) -> bool {
    let Some((owner, name)) = repo.split_once('/') else {
        return false;
    };
    !owner.starts_with('-')
        && summonr_core::text::is_name(owner, summonr_core::text::Charset::Dash)
        && !matches!(name, "." | "..")
        && summonr_core::text::is_name(name, summonr_core::text::Charset::Dotted)
}

fn local(input: &str) -> bool {
    let windows = cfg!(windows)
        && (input.starts_with(".\\")
            || input.starts_with("..\\")
            || input.get(1..3) == Some(":\\"));
    ["./", "../", "/", "~/"]
        .iter()
        .any(|prefix| input.starts_with(prefix))
        || windows
}

fn in_place(input: &str) -> Result<Origin> {
    let written = match input.strip_prefix("~/") {
        Some(rest) => summonr_core::dirs::home_dir()
            .ok_or_else(|| Error::Config("no home folder for `~`".to_string()))?
            .join(rest),
        None => PathBuf::from(input),
    };
    let path = written
        .canonicalize()
        .map_err(|error| Error::Config(format!("{input}: {error}")))?;
    if path.is_dir() {
        return Ok(Origin::Directory { path });
    }
    if super::super::is_json(&path) {
        return Ok(Origin::File { path });
    }
    Err(Error::Config(format!(
        "{input} is neither a directory nor a .json file"
    )))
}

fn web(input: &str, body: &str, reference: Option<String>) -> Origin {
    let git = |url: String| Origin::Git {
        url,
        reference: reference.clone(),
    };
    if body.ends_with(".git") || body.contains("/_git/") {
        return git(body.to_string());
    }
    let host = body
        .split_once("://")
        .map_or("", |(_, rest)| rest.split('/').next().unwrap_or(""));
    let has_path = body
        .split_once("://")
        .is_some_and(|(_, rest)| rest.trim_end_matches('/').contains('/'));
    if GIT_HOSTS.contains(&host) && has_path {
        return git(format!("{}.git", body.trim_end_matches('/')));
    }
    Origin::Url {
        url: input.to_string(),
    }
}

/// Whether `body` is an scp-style `user@host:path` git address.
pub(super) fn ssh(body: &str) -> bool {
    let Some((user_host, path)) = body.split_once(':') else {
        return false;
    };
    user_host.contains('@') && !user_host.contains('/') && !path.is_empty()
}

#[cfg(test)]
#[path = "origin_tests.rs"]
mod tests;
