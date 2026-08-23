use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::{Record, SessionInfo, parse_record, sessions_dir};
use crate::durable::journal;
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
    let exact = sessions_dir()?.join(format!("{id}.jsonl"));
    if exact.is_file() {
        return Ok(exact);
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

/// All sessions under `$MAGE_HOME/sessions-v2` then `$MAGE_HOME/sessions`, oldest first.
pub fn list() -> Result<Vec<SessionInfo>> {
    let mut paths = Vec::new();
    let v2 = crate::prompt::mage_home()?.join("sessions-v2");
    if v2.is_dir() {
        paths.extend(jsonl_paths(&v2)?);
    }
    let dir = sessions_dir()?;
    if dir.is_dir() {
        paths.extend(jsonl_paths(&dir)?);
    }
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
    if journal::parse_path(path).is_some() {
        return peek_format2(path);
    }
    let Ok(file) = File::open(path) else {
        return (None, None, String::new());
    };
    let mut cwd = None;
    let mut model = None;
    let mut preview = String::new();
    for line in BufReader::new(file)
        .lines()
        .map_while(std::result::Result::ok)
    {
        let Ok(record) = parse_record(&line) else {
            continue;
        };
        match record {
            Record::Header {
                cwd: header_cwd,
                model: header_model,
                ..
            } => {
                cwd = Some(header_cwd);
                model = Some(header_model);
            }
            Record::User { text, .. } if preview.is_empty() => {
                preview = text.chars().take(72).collect();
            }
            _ => {}
        }
        if cwd.is_some() && !preview.is_empty() {
            break;
        }
    }
    (cwd, model, preview)
}

fn peek_format2(path: &Path) -> (Option<String>, Option<String>, String) {
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
