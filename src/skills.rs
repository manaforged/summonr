use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::prompt::summonr_home;

const SKILL: &str = "SKILL.md";
const FENCE: &str = "---";

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
    for dir in plugin_dirs() {
        collect(Some(dir), &mut rows);
    }
    if rows.is_empty() {
        return None;
    }
    rows.sort();
    rows.dedup_by(|left, right| left.0 == right.0);
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
    for dir in plugin_dirs() {
        candidates.push(dir.join(name).join("SKILL.md"));
    }
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
        rows.push((name, summary(std::io::BufReader::new(handle))));
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

fn summary(reader: impl std::io::BufRead) -> String {
    let mut lines = reader.lines().map_while(std::result::Result::ok);
    let Some(first) = lines
        .by_ref()
        .map(|line| line.trim().to_string())
        .find(|line| !line.is_empty())
    else {
        return String::new();
    };
    if first != FENCE {
        return heading(&first);
    }
    let mut description = None;
    for line in lines.by_ref() {
        let line = line.trim();
        if line == FENCE {
            return description
                .filter(|text: &String| !text.is_empty())
                .unwrap_or_else(|| heading(FENCE));
        }
        if description.is_none()
            && let Some(text) = line.strip_prefix("description:")
        {
            description = Some(text.trim().trim_matches(['"', '\'']).to_string());
        }
    }
    heading(FENCE)
}

fn heading(line: &str) -> String {
    line.trim_start_matches('#')
        .trim()
        .chars()
        .take(80)
        .collect()
}

fn plugin_dirs() -> Vec<PathBuf> {
    crate::plugins::enabled()
        .iter()
        .flat_map(crate::plugins::Plugin::skill_dirs)
        .collect()
}
