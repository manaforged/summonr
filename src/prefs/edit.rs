use std::path::Path;

use super::load;

pub(crate) fn banned_edit(
    dest: &Path,
    before: &str,
    after: &str,
    allowed: &[&str],
) -> Option<(&'static str, String)> {
    let prefs = load().unwrap_or_default();
    if prefs.ban_tests && !allowed.contains(&"tests") && test_surface(dest, before, after) {
        return Some(("tests", dest.display().to_string()));
    }
    if prefs.ban_comments
        && !allowed.contains(&"comments")
        && let Some(line) = added_comment(dest, before, after)
    {
        return Some(("comments", line));
    }
    None
}

fn test_surface(dest: &Path, before: &str, after: &str) -> bool {
    let name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let path = dest.to_string_lossy();
    if name.ends_with("_tests.rs") || name.starts_with("test_") || path.contains("/tests/") {
        return true;
    }
    ["#[test]", "#[cfg(test)]"]
        .iter()
        .any(|marker| after.contains(marker) && !before.contains(marker))
}

fn added_comment(dest: &Path, before: &str, after: &str) -> Option<String> {
    let extension = dest
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    let markers: &[&str] = if matches!(
        extension,
        "rs" | "ts" | "tsx" | "js" | "go" | "wgsl" | "glsl"
    ) {
        &["//", "/*"]
    } else if matches!(extension, "py" | "sh" | "bash" | "zsh") {
        &["#"]
    } else {
        return None;
    };
    let have: std::collections::HashSet<&str> = before.lines().collect();
    for line in after.lines() {
        let text = line.trim_start();
        let directive =
            text.starts_with("#!") || text.contains("coding:") || text.contains("coding=");
        if !directive
            && markers.iter().any(|marker| text.starts_with(marker))
            && !have.contains(line)
        {
            return Some(text.chars().take(80).collect());
        }
    }
    None
}
