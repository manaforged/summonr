use std::io::BufRead;
use std::path::Path;

use summonr_core::text::{FENCE, Frontmatter};

pub(super) fn about(file: &Path) -> String {
    std::fs::File::open(file)
        .ok()
        .and_then(|handle| head(std::io::BufReader::new(handle)))
        .map(|text| summary(&text))
        .unwrap_or_default()
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
        Some(_) => Frontmatter::parse(text)
            .get("description")
            .map_or_else(|| heading(FENCE), summonr_core::text::squash),
    }
}

fn heading(line: &str) -> String {
    line.trim_start_matches('#')
        .trim()
        .chars()
        .take(80)
        .collect()
}
