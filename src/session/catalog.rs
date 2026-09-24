use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Prompt, SessionInfo, user_text};
use crate::durable::id::SessionId;
use crate::durable::log::{self, Line};
use crate::durable::store;
use crate::error::{Error, Result};
use crate::prompt::summonr_home;

pub fn last_for(workspace: &Path) -> Result<Option<PathBuf>> {
    last_in(&crate::prompt::summonr_home()?, workspace)
}

pub(crate) fn previous_for(workspace: &Path, current: &str) -> Result<Option<SessionInfo>> {
    previous_in(&crate::prompt::summonr_home()?, workspace, current)
}

pub(crate) fn previous_in(
    home: &Path,
    workspace: &Path,
    current: &str,
) -> Result<Option<SessionInfo>> {
    let want = roots(workspace);
    let mut matches: Vec<SessionInfo> = list_in(home)?
        .into_iter()
        .filter(|info| {
            info.id != current
                && info.depth == 0
                && info
                    .cwd
                    .as_deref()
                    .is_some_and(|cwd| want.iter().any(|root| root == cwd))
        })
        .collect();
    Ok(matches.pop())
}

pub(crate) fn last_in(home: &Path, workspace: &Path) -> Result<Option<PathBuf>> {
    let want = roots(workspace);
    let mut matches: Vec<SessionInfo> = list_in(home)?
        .into_iter()
        .filter(|info| {
            info.depth == 0
                && info
                    .cwd
                    .as_deref()
                    .is_some_and(|cwd| want.iter().any(|root| root == cwd))
        })
        .collect();
    Ok(matches.pop().map(|info| info.path))
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
    let short = Some(id.trim_start_matches('0')).filter(|short| !short.is_empty());
    short.map_or(id, |short| short.get(..12).unwrap_or(short))
}

pub(crate) fn preview(text: &str) -> String {
    user_text(text).chars().take(72).collect()
}

pub(crate) fn name_of(path: &Path) -> Option<String> {
    std::fs::read_to_string(log::name_path(path))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|name| !name.is_empty())
}

pub fn set_name(path: &Path, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() || name.contains('\n') {
        return Err(Error::Config(
            "a session name is one non-empty line".to_string(),
        ));
    }
    store::write(&log::name_path(path), format!("{name}\n"))?;
    Ok(())
}

pub fn export(path: &Path) -> Result<String> {
    let mut out = String::new();
    for line in log::read(path)? {
        match line {
            Line::Session {
                id,
                model,
                workspace,
                ..
            } => {
                out.push_str(&format!(
                    "# session {} · {model} · {}\n",
                    short(&id.to_string()),
                    workspace.display()
                ));
                if let Some(name) = name_of(path) {
                    out.push_str(&format!("name: {name}\n"));
                }
                out.push('\n');
            }
            Line::User {
                text,
                source,
                start,
                ..
            } => {
                if start {
                    let text = source.as_deref().unwrap_or_else(|| user_text(&text));
                    out.push_str(&format!("## You\n\n{text}\n\n"));
                } else {
                    out.push_str(&format!("> {}\n\n", text.lines().next().unwrap_or("")));
                }
            }
            Line::Assistant {
                text, tool_calls, ..
            } => {
                if !text.trim().is_empty() {
                    out.push_str(&format!("## Assistant\n\n{text}\n\n"));
                }
                for call in &tool_calls {
                    let args: String = call.function.arguments.chars().take(100).collect();
                    out.push_str(&format!("- `{}` {args}\n", call.function.name));
                }
                if !tool_calls.is_empty() {
                    out.push('\n');
                }
            }
            Line::Interrupted { text, .. } => {
                out.push_str(&format!("## Assistant (interrupted)\n\n{text}\n\n"));
            }
            Line::Tool { text, success, .. } => {
                let shown: Vec<&str> = text.lines().take(12).collect();
                out.push_str(&format!(
                    "```\n{}{}\n```\n{}\n",
                    shown.join("\n"),
                    if text.lines().count() > 12 {
                        "\n…"
                    } else {
                        ""
                    },
                    if success { "" } else { "(failed)\n" }
                ));
            }
            Line::Receipt { text, .. } => {
                out.push_str(&format!("> {}\n\n", text.replace('\n', "\n> ")));
            }
            Line::Summary { text, .. } => {
                out.push_str(&format!("## Compacted\n\n{text}\n\n"));
            }
            Line::Clear => out.push_str("---\n\n"),
            Line::Tools { .. }
            | Line::Budget { .. }
            | Line::Workflow { .. }
            | Line::Child { .. }
            | Line::Charge { .. }
            | Line::Rewrite { .. }
            | Line::Close { .. } => {}
        }
    }
    Ok(out)
}

pub fn resolve_id(id: &str) -> Result<PathBuf> {
    resolve_in(&summonr_home()?, id)
}

pub(crate) fn resolve_in(home: &Path, id: &str) -> Result<PathBuf> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::Config("session id is empty".to_string()));
    }
    if let Some(named) = list_in(home)?.into_iter().rev().find(|info| {
        info.name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(id))
    }) {
        return Ok(named.path);
    }
    if let Ok(session) = id.parse() {
        let exact = log::session_path(home, session);
        if exact.is_file() {
            return Ok(exact);
        }
    }
    let hits: Vec<PathBuf> = list_in(home)?
        .into_iter()
        .filter(|info| prefix(&info.id, id) || (id.len() >= 5 && info.id.ends_with(id)))
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

pub(crate) fn remove(id: &str) -> Result<String> {
    remove_in(&crate::prompt::summonr_home()?, id)
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
                    "session id `{id}` is ambiguous — pass more of the id"
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
    list_in(&crate::prompt::summonr_home()?)
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
                    text: source.unwrap_or_else(|| user_text(&text).to_string()),
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

fn peek(path: &Path) -> (Option<String>, Option<String>, String, Option<String>, u8) {
    let Ok(lines) = log::scan(path) else {
        return (None, None, String::new(), None, 0);
    };
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
                model = Some(name);
                parent = recorded_parent.map(|id| id.to_string());
                depth = recorded_depth;
            }
            Line::User { text, source, .. } if !text.is_empty() => {
                return (
                    cwd,
                    model,
                    preview(source.as_deref().unwrap_or(&text)),
                    parent,
                    depth,
                );
            }
            _ => {}
        }
    }
    (cwd, model, String::new(), parent, depth)
}
