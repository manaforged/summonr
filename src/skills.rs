use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::prompt::summonr_home;

#[must_use]
pub(crate) fn catalog(workspace: &Path) -> Option<String> {
    let mut rows = Vec::new();
    collect(
        summonr_home()
            .ok()
            .as_deref()
            .map(|home| home.join("skills")),
        &mut rows,
    );
    collect(Some(workspace.join(".summonr/skills")), &mut rows);
    if rows.is_empty() {
        return None;
    }
    rows.sort();
    rows.dedup_by(|left, right| left.0 == right.0);
    let mut out = String::from(
        "Skills (load with the skill tool, then follow the file). Catalog only — bodies stay on disk:",
    );
    for (name, summary) in rows {
        out.push_str("\n- ");
        out.push_str(&name);
        if !summary.is_empty() {
            out.push_str(" — ");
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
    let mut candidates = Vec::new();
    if let Ok(home) = summonr_home() {
        candidates.push(home.join("skills").join(name).join("SKILL.md"));
        candidates.push(home.join("skills").join(format!("{name}.md")));
    }
    candidates.push(
        workspace
            .join(".summonr/skills")
            .join(name)
            .join("SKILL.md"),
    );
    candidates.push(workspace.join(".summonr/skills").join(format!("{name}.md")));
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

fn collect(root: Option<PathBuf>, rows: &mut Vec<(String, String)>) {
    let Some(root) = root else {
        return;
    };
    if !root.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        let Some(name) = skill_name(&path) else {
            continue;
        };
        let file = if path.is_dir() {
            path.join("SKILL.md")
        } else {
            path
        };
        let body = std::fs::read_to_string(file).unwrap_or_default();
        let summary = first_line(&body);
        rows.push((name, summary));
    }
}

fn skill_name(path: &Path) -> Option<String> {
    if path.is_dir() {
        let skill = path.join("SKILL.md");
        if skill.is_file() {
            return path.file_name()?.to_str().map(str::to_string);
        }
        return None;
    }
    if path.extension().is_some_and(|ext| ext == "md") {
        return path.file_stem()?.to_str().map(str::to_string);
    }
    None
}

fn first_line(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .trim_start_matches('#')
        .trim()
        .chars()
        .take(80)
        .collect()
}
