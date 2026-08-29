use std::io::BufRead;
use std::path::{Path, PathBuf};

use super::SessionInfo;
use crate::durable::id::SessionId;
use crate::durable::log::{self, Line};
use crate::error::{Error, Result};

pub fn last_for(workspace: &Path) -> Result<Option<PathBuf>> {
    last_in(&crate::prompt::mage_home()?, workspace)
}

pub(crate) fn last_in(home: &Path, workspace: &Path) -> Result<Option<PathBuf>> {
    let want = roots(workspace);
    let mut matches: Vec<SessionInfo> = list_in(home)?
        .into_iter()
        .filter(|info| {
            info.cwd
                .as_deref()
                .is_some_and(|cwd| want.iter().any(|root| root == cwd))
        })
        .collect();
    Ok(matches.pop().map(|info| info.path))
}

fn roots(workspace: &Path) -> Vec<String> {
    let mut want = vec![workspace.display().to_string()];
    let Ok(canonical) = workspace.canonicalize() else {
        return want;
    };
    want.push(canonical.display().to_string());
    if let Ok(primary) = crate::worktree::primary(&canonical) {
        want.push(primary.display().to_string());
        if let Ok(trees) = crate::worktree::list(&primary) {
            for tree in trees {
                want.push(tree.path.display().to_string());
            }
        }
    }
    want
}

pub fn resolve_id(id: &str) -> Result<PathBuf> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::Config("session id is empty".to_string()));
    }
    let home = crate::prompt::mage_home()?;
    if let Ok(session) = id.parse() {
        let exact = log::session_path(&home, session);
        if exact.is_file() {
            return Ok(exact);
        }
    }
    let hits: Vec<PathBuf> = list_in(&home)?
        .into_iter()
        .filter(|info| info.id == id || info.id.starts_with(id))
        .map(|info| info.path)
        .collect();
    match hits.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(Error::Config(format!("no session `{id}`"))),
        _ => Err(Error::Config(format!(
            "session id `{id}` is ambiguous — pass more of the id"
        ))),
    }
}

pub fn list() -> Result<Vec<SessionInfo>> {
    list_in(&crate::prompt::mage_home()?)
}

pub(crate) fn list_in(home: &Path) -> Result<Vec<SessionInfo>> {
    let dir = log::sessions_dir(home);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let id: SessionId = entry.file_name().to_str()?.parse().ok()?;
            Some(log::session_path(home, id))
        })
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    Ok(paths.into_iter().map(info).collect())
}

fn info(path: PathBuf) -> SessionInfo {
    let id = log::parse_path(&path)
        .map(|session| session.to_string())
        .unwrap_or_default();
    let (cwd, model, preview) = peek(&path);
    SessionInfo {
        id,
        path,
        cwd,
        model,
        preview,
    }
}

fn peek(path: &Path) -> (Option<String>, Option<String>, String) {
    let Ok(file) = std::fs::File::open(path) else {
        return (None, None, String::new());
    };
    let mut cwd = None;
    let mut model = None;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        match serde_json::from_str(&line) {
            Ok(Line::Session {
                workspace,
                model: name,
                ..
            }) => {
                cwd = Some(workspace.display().to_string());
                model = Some(name);
            }
            Ok(Line::User { text, .. }) if !text.is_empty() => {
                return (cwd, model, text.chars().take(72).collect());
            }
            _ => {}
        }
    }
    (cwd, model, String::new())
}
