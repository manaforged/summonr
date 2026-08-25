use std::path::{Path, PathBuf};

use super::SessionInfo;
use crate::durable::journal;
use crate::durable::observe::workspace_identity_of;
use crate::durable::project;
use crate::error::{Error, Result};
use crate::provider::Role;

/// Sessions whose header cwd matches `workspace` or a `summon/*` worktree
/// of the same repository, newest first.
pub fn last_for(workspace: &Path) -> Result<Option<PathBuf>> {
    let want = roots(workspace);
    let mut matches: Vec<SessionInfo> = list()?
        .into_iter()
        .filter(|info| {
            info.cwd
                .as_deref()
                .is_some_and(|cwd| want.iter().any(|root| root == cwd))
                || format2_workspace_matches(&info.path, &want)
        })
        .collect();
    Ok(matches.pop().map(|info| info.path))
}

pub(super) fn format2_workspace_matches(path: &Path, roots: &[String]) -> bool {
    let Some(session) = journal::parse_path(path) else {
        return false;
    };
    let Ok(state) = journal::inspect(path) else {
        return false;
    };
    if state.session() != Some(session) {
        return false;
    }
    let Some(scope) = state
        .default_scope
        .and_then(|scope| state.scopes.get(&scope))
    else {
        return false;
    };
    roots.iter().any(|root| {
        workspace_identity_of(Path::new(root))
            .is_ok_and(|identity| identity.object() == scope.workspace().object())
    })
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

/// Resolve a session id or unique prefix to a log path.
pub fn resolve_id(id: &str) -> Result<PathBuf> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::Config("session id is empty".to_string()));
    }
    let v2 = crate::prompt::mage_home()?
        .join("sessions-v2")
        .join(format!("{id}.jsonl"));
    if v2.is_file() {
        return Ok(v2);
    }
    let hits: Vec<PathBuf> = list()?
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

/// All format-2 sessions, oldest first.
pub fn list() -> Result<Vec<SessionInfo>> {
    let v2 = crate::prompt::mage_home()?.join("sessions-v2");
    let mut paths = if v2.is_dir() {
        jsonl_paths(&v2)?
    } else {
        Vec::new()
    };
    paths.sort();
    Ok(paths.into_iter().map(info).collect())
}

fn jsonl_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    Ok(std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect())
}

fn info(path: PathBuf) -> SessionInfo {
    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("session")
        .to_string();
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
    let Ok(state) = journal::inspect(path) else {
        return (None, None, String::new());
    };
    let replay = project::replay(&state);
    let preview = replay
        .history
        .iter()
        .find(|message| message.role == Role::User && !message.content.is_empty())
        .map(|message| message.content.chars().take(72).collect())
        .unwrap_or_default();
    (None, replay.model, preview)
}
