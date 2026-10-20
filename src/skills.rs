use std::io::BufRead;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::prompt::summonr_home;

const SKILL: &str = "SKILL.md";
use crate::text::FENCE;

#[must_use]
pub(crate) fn list(workspace: &Path) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for root in roots(workspace) {
        collect(root, &mut rows);
    }
    rows.retain(|(name, _)| valid(name));
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    rows.dedup_by(|left, right| left.0 == right.0);
    rows
}

#[must_use]
pub(crate) fn catalog(workspace: &Path) -> Option<String> {
    let rows = list(workspace);
    if rows.is_empty() {
        return None;
    }
    let mut out = String::from(
        "Skills (load with the skill tool, then follow the file). Catalog only; bodies stay on disk:",
    );
    for (name, summary) in rows {
        out.push_str("\n- ");
        out.push_str(&name);
        if !summary.is_empty() {
            out.push_str(": ");
            out.push_str(&summary);
        }
    }
    Some(out)
}

pub(crate) fn load(workspace: &Path, name: &str) -> Result<String> {
    let name = name.trim();
    if !valid(name) {
        return Err(Error::Tool(
            "skill names are letters, digits, `-`, and `_` only".to_string(),
        ));
    }
    let candidates = roots(workspace)
        .into_iter()
        .flat_map(|root| [root.join(name).join(SKILL), root.join(format!("{name}.md"))]);
    for path in candidates {
        if let Ok(text) = std::fs::read_to_string(&path)
            && !text.trim().is_empty()
        {
            return Ok(text);
        }
    }
    Err(Error::Tool(format!("skill `{name}` is not installed")))
}

pub(crate) fn valid(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn collect(root: PathBuf, rows: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        let dir = std::fs::metadata(&path).is_ok_and(|meta| meta.is_dir());
        let Some((name, file)) = skill_file(path, dir) else {
            continue;
        };
        let Ok(handle) = std::fs::File::open(&file) else {
            if !dir {
                rows.push((name, String::new()));
            }
            continue;
        };
        rows.push((
            name,
            head(std::io::BufReader::new(handle))
                .map(|text| summary(&text))
                .unwrap_or_default(),
        ));
    }
}

fn skill_file(path: PathBuf, dir: bool) -> Option<(String, PathBuf)> {
    if dir {
        let name = path.file_name()?.to_str()?.to_string();
        return Some((name, path.join(SKILL)));
    }
    if path.extension().is_some_and(|ext| ext == "md") {
        let name = path.file_stem()?.to_str()?.to_string();
        return Some((name, path));
    }
    None
}

fn head(mut reader: impl BufRead) -> Option<String> {
    let mut text = String::new();
    let mut line = Vec::new();
    let mut fenced = false;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).ok()? == 0 {
            return Some(text);
        }
        let piece = std::str::from_utf8(&line).ok()?;
        text.push_str(piece);
        let trimmed = piece.trim();
        if trimmed.is_empty() {
            continue;
        }
        if fenced || trimmed != FENCE {
            if !fenced || piece.trim_end() == FENCE {
                return Some(text);
            }
            continue;
        }
        fenced = true;
    }
}

fn summary(text: &str) -> String {
    let first = text.lines().map(str::trim).find(|line| !line.is_empty());
    match first {
        None => String::new(),
        Some(line) if line != FENCE => heading(line),
        Some(_) => crate::text::Frontmatter::parse(text)
            .get("description")
            .map_or_else(|| heading(FENCE), crate::text::squash),
    }
}

fn heading(line: &str) -> String {
    line.trim_start_matches('#')
        .trim()
        .chars()
        .take(80)
        .collect()
}

fn roots(workspace: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = summonr_home()
        .ok()
        .map(|home| home.join("skills"))
        .into_iter()
        .collect();
    if let Some(home) = crate::prompt::user_home() {
        roots.push(home.join(".claude/skills"));
        roots.push(home.join(".agents/skills"));
    }
    roots.push(workspace.join(".summonr/skills"));
    roots.push(workspace.join(".claude/skills"));
    roots.extend(plugin_dirs());
    roots
}

fn plugin_dirs() -> Vec<PathBuf> {
    crate::plugins::enabled()
        .iter()
        .flat_map(crate::plugins::Plugin::skill_dirs)
        .collect()
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;
