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

pub(crate) fn prefix(id: &str, key: &str) -> bool {
    let key = key.trim_start_matches('0');
    id == key || id.trim_start_matches('0').starts_with(key)
}

#[must_use]
pub fn short(id: &str) -> &str {
    let short = id.trim_start_matches('0');
    if short.is_empty() { id } else { short }
}

pub(crate) fn preview(text: &str) -> String {
    text.split("\n\nHarness state:")
        .next()
        .unwrap_or(text)
        .chars()
        .take(72)
        .collect()
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
    std::fs::write(log::name_path(path), format!("{name}\n"))?;
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
            Line::User { text, start, .. } => {
                if start {
                    let text = text.split("\n\nHarness state:").next().unwrap_or(&text);
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
            Line::Receipt { text } => {
                out.push_str(&format!("> {}\n\n", text.replace('\n', "\n> ")));
            }
            Line::Summary { text, .. } => {
                out.push_str(&format!("## Compacted\n\n{text}\n\n"));
            }
            Line::Clear => out.push_str("---\n\n"),
            Line::Budget { .. }
            | Line::Workflow { .. }
            | Line::Charge { .. }
            | Line::Close { .. } => {}
        }
    }
    Ok(out)
}

pub fn resolve_id(id: &str) -> Result<PathBuf> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::Config("session id is empty".to_string()));
    }
    let home = crate::prompt::mage_home()?;
    if let Some(named) = list_in(&home)?.into_iter().rev().find(|info| {
        info.name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(id))
    }) {
        return Ok(named.path);
    }
    if let Ok(session) = id.parse() {
        let exact = log::session_path(&home, session);
        if exact.is_file() {
            return Ok(exact);
        }
    }
    let hits: Vec<PathBuf> = list_in(&home)?
        .into_iter()
        .filter(|info| prefix(&info.id, id))
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
    let name = name_of(&path);
    SessionInfo {
        id,
        path,
        cwd,
        model,
        preview,
        name,
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
                return (cwd, model, preview(&text));
            }
            _ => {}
        }
    }
    (cwd, model, String::new())
}
