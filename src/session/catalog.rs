use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Prompt, SessionInfo, user_text};
use crate::durable::id::SessionId;
use crate::durable::log::{self, Line};
use crate::durable::store;
use crate::error::{Error, Result};
use crate::prompt::summonr_home;

pub fn last_for(workspace: &Path) -> Result<Option<SessionInfo>> {
    latest_in(&summonr_home()?, workspace, None)
}

pub(crate) fn previous_for(workspace: &Path, current: &str) -> Result<Option<SessionInfo>> {
    latest_in(&summonr_home()?, workspace, Some(current))
}

pub(crate) fn latest_in(
    home: &Path,
    workspace: &Path,
    skip: Option<&str>,
) -> Result<Option<SessionInfo>> {
    let want = roots(workspace);
    Ok(list_in(home)?.into_iter().rev().find(|info| {
        skip != Some(info.id.as_str())
            && info.depth == 0
            && info
                .cwd
                .as_deref()
                .is_some_and(|cwd| want.iter().any(|root| root == cwd))
    }))
}

pub(crate) fn roots(workspace: &Path) -> Vec<String> {
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

pub(crate) fn prefix(id: &str, key: &str) -> bool {
    let key = key.trim_start_matches('0');
    id == key || id.trim_start_matches('0').starts_with(key)
}

#[must_use]
pub fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

const PREVIEW_WIDTH: usize = 72;

pub(crate) fn preview(text: &str, source: Option<&str>) -> String {
    crate::text::ellipsize(user_text(text, source), PREVIEW_WIDTH)
}

pub(crate) fn name_of(path: &Path) -> Option<String> {
    std::fs::read_to_string(log::name_path(path))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|name| !name.is_empty())
}

pub(crate) fn set_name(path: &Path, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() || name.contains('\n') {
        return Err(Error::Config(
            "a session name is one non-empty line".to_string(),
        ));
    }
    store::write(&log::name_path(path), format!("{name}\n"))?;
    Ok(())
}

pub fn resolve_id(id: &str) -> Result<PathBuf> {
    resolve_in(&summonr_home()?, id)
}

pub(crate) fn resolve_in(home: &Path, id: &str) -> Result<PathBuf> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::Config("session id is empty".to_string()));
    }
    let sessions = list_in(home)?;
    if let Some(named) = sessions.iter().rev().find(|info| {
        info.name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(id))
    }) {
        return Ok(named.path.clone());
    }
    if let Ok(session) = id.parse() {
        let exact = log::session_path(home, session);
        if exact.is_file() {
            return Ok(exact);
        }
    }
    let hits: Vec<PathBuf> = sessions
        .into_iter()
        .filter(|info| prefix(&info.id, id) || (id.len() >= 5 && info.id.ends_with(id)))
        .map(|info| info.path)
        .collect();
    match hits.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(Error::Config(format!("no session `{id}`"))),
        _ => Err(Error::Config(format!(
            "session id `{id}` is ambiguous; pass more of the id"
        ))),
    }
}

pub(crate) fn remove(id: &str) -> Result<String> {
    remove_in(&summonr_home()?, id)
}

pub(crate) fn remove_in(home: &Path, id: &str) -> Result<String> {
    let id = id.trim();
    let exact = log::sessions_dir(home).join(id);
    let dir = if !id.is_empty() && exact.is_dir() {
        exact
    } else {
        let hits: Vec<PathBuf> = list_in(home)?
            .into_iter()
            .filter(|info| prefix(&info.id, id))
            .map(|info| info.path)
            .collect();
        match hits.as_slice() {
            [only] => only
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| Error::Config(format!("session `{id}` has no directory")))?,
            [] => return Err(Error::Config(format!("no session `{id}`"))),
            _ => {
                return Err(Error::Config(format!(
                    "session id `{id}` is ambiguous; pass more of the id"
                )));
            }
        }
    };
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    std::fs::remove_dir_all(&dir)?;
    Ok(name)
}

pub fn list() -> Result<Vec<SessionInfo>> {
    list_in(&summonr_home()?)
}

const PROMPT_SESSIONS: usize = 64;

pub(crate) fn prompts(scope: Option<&str>) -> Result<(Vec<Prompt>, Vec<String>)> {
    prompts_in(&summonr_home()?, scope)
}

pub(crate) fn prompts_in(home: &Path, scope: Option<&str>) -> Result<(Vec<Prompt>, Vec<String>)> {
    let mut prompts = Vec::new();
    let mut skipped = Vec::new();
    let sessions = match scope {
        Some(id) => {
            let id = id
                .parse()
                .map_err(|error| Error::Config(format!("Invalid session ID: {error}")))?;
            vec![info(log::session_path(home, id))]
        }
        None => list_in(home)?,
    };
    for session in sessions
        .into_iter()
        .rev()
        .filter(|info| info.depth == 0)
        .take(PROMPT_SESSIONS)
    {
        let lines = match log::read(&session.path) {
            Ok(lines) => lines,
            Err(error) => {
                let error = format!("Cannot read session {}: {error}", session.path.display());
                if scope.is_some() {
                    return Err(Error::Config(error));
                }
                skipped.push(error);
                continue;
            }
        };
        let session = Arc::new(session);
        let mut saved: Vec<Prompt> = Vec::new();
        for line in lines {
            match line {
                Line::User {
                    text,
                    source,
                    images,
                    start: true,
                } => saved.push(Prompt {
                    session: Arc::clone(&session),
                    turn: saved.len(),
                    text: user_text(&text, source.as_deref()).to_string(),
                    images: images.len(),
                    closed: false,
                }),
                Line::Close { .. } => {
                    if let Some(prompt) = saved.last_mut() {
                        prompt.closed = true;
                    }
                }
                _ => {}
            }
        }
        saved.reverse();
        prompts.extend(saved);
    }
    Ok((prompts, skipped))
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
    let (cwd, model, preview, parent, depth) = peek(&path);
    let name = name_of(&path);
    SessionInfo {
        id,
        path,
        cwd,
        model,
        preview,
        name,
        parent,
        depth,
    }
}

type Peek = (Option<String>, Option<String>, String, Option<String>, u8);

fn peek(path: &Path) -> Peek {
    const PEEK_LINES: usize = 64;
    if let Ok(head) = log::head(path, PEEK_LINES) {
        let (info, found) = peek_lines(&head);
        if found {
            return info;
        }
    }
    match log::scan(path) {
        Ok(lines) => {
            let lines: Vec<Line> = lines.collect();
            peek_lines(&lines).0
        }
        Err(_) => (None, None, String::new(), None, 0),
    }
}

fn peek_lines(lines: &[Line]) -> (Peek, bool) {
    let mut cwd = None;
    let mut model = None;
    let mut parent = None;
    let mut depth = 0;
    for line in lines {
        match line {
            Line::Session {
                workspace,
                model: name,
                parent: recorded_parent,
                depth: recorded_depth,
                ..
            } => {
                cwd = Some(workspace.display().to_string());
                model = Some(name.clone());
                parent = recorded_parent.map(|id| id.to_string());
                depth = *recorded_depth;
            }
            Line::User { text, source, .. } if !text.is_empty() => {
                let info = (cwd, model, preview(text, source.as_deref()), parent, depth);
                return (info, true);
            }
            _ => {}
        }
    }
    ((cwd, model, String::new(), parent, depth), false)
}
