use std::collections::HashSet;
use std::path::{Path, PathBuf};

use summonr_core::markdown::{Fence, Mention, targets};

const MAX_HOPS: usize = 4;

/// Reads instruction files once each and expands their `@path` imports.
pub(super) struct Loader<'a> {
    seen: HashSet<PathBuf>,
    home: Option<&'a Path>,
}

impl<'a> Loader<'a> {
    pub(super) fn new(home: Option<&'a Path>) -> Self {
        Self {
            seen: HashSet::new(),
            home,
        }
    }

    /// Marks `path` as loaded; false when it, or the file it links to, already was.
    pub(super) fn claim(&mut self, path: &Path) -> bool {
        self.seen
            .insert(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
    }

    /// The text of `path` with its `@path` imports expanded, or `None` when it is unreadable or blank.
    pub(super) fn expand(&mut self, path: &Path) -> Option<String> {
        self.read(path, 0)
    }

    fn read(&mut self, path: &Path, hops: usize) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        let dir = path.parent().unwrap_or(path);
        let mut fence = Fence::default();
        let mut lines = Vec::new();
        for line in text.lines() {
            if fence.code(line) {
                lines.push(line.to_string());
                continue;
            }
            let found: Vec<Mention> = if hops < MAX_HOPS {
                targets(line)
                    .into_iter()
                    .filter(|mention| !mention.quoted && !mention.path.starts_with('\''))
                    .collect()
            } else {
                Vec::new()
            };
            self.line(&mut lines, line, dir, &found, hops);
        }
        let text = lines.join("\n");
        (!text.trim().is_empty()).then_some(text)
    }

    fn line(
        &mut self,
        lines: &mut Vec<String>,
        line: &str,
        dir: &Path,
        found: &[Mention],
        hops: usize,
    ) {
        let whole = matches!(found, [mention] if line[mention.span.clone()] == *line.trim());
        let mut texts = Vec::new();
        let mut missing = false;
        for mention in found {
            let target = self.resolve(dir, &mention.path);
            if !self.claim(&target) {
                continue;
            }
            match self.read(&target, hops + 1) {
                Some(text) => texts.push(text),
                None => missing = true,
            }
        }
        if !whole || missing || found.is_empty() {
            lines.push(line.to_string());
        }
        lines.extend(texts);
    }

    fn resolve(&self, dir: &Path, raw: &str) -> PathBuf {
        match (raw.strip_prefix("~/"), self.home) {
            (Some(rest), Some(home)) => home.join(rest),
            (None, _) | (Some(_), None) => dir.join(raw),
        }
    }
}
