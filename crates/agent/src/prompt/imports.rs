use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

const MAX_HOPS: usize = 4;
const FENCE_CHARS: [char; 2] = ['`', '~'];
const FENCE_MIN: usize = 3;
const FENCE_INDENT: usize = 3;

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
        let mut fence = None;
        let mut lines = Vec::new();
        for line in text.lines() {
            if let Some(mark) = fence_mark(line) {
                fence = match fence {
                    None => Some(mark),
                    Some((open, width)) if mark.0 == open && mark.1 >= width => None,
                    open @ Some(_) => open,
                };
                lines.push(line.to_string());
                continue;
            }
            let found = if fence.is_none() && hops < MAX_HOPS {
                targets(line)
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
        found: &[(Range<usize>, String)],
        hops: usize,
    ) {
        let whole = matches!(found, [(span, _)] if line[span.clone()] == *line.trim());
        let mut texts = Vec::new();
        let mut missing = false;
        for (_, raw) in found {
            let target = self.resolve(dir, raw);
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

fn fence_mark(line: &str) -> Option<(char, usize)> {
    let body = line.trim_start_matches(' ');
    if line.len() - body.len() > FENCE_INDENT {
        return None;
    }
    let open = body.chars().next().filter(|c| FENCE_CHARS.contains(c))?;
    let width = body.chars().take_while(|c| *c == open).count();
    (width >= FENCE_MIN).then_some((open, width))
}

fn targets(line: &str) -> Vec<(Range<usize>, String)> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'`' {
            at = span_end(line, at);
        } else if bytes[at] == b'@' && (at == 0 || bytes[at - 1].is_ascii_whitespace()) {
            let (path, len) = token(&line[at + 1..]);
            if !path.is_empty() && !path.starts_with(['"', '\'']) {
                out.push((at..at + 1 + len, path));
            }
            at += 1 + len;
        } else {
            at += 1;
        }
    }
    out
}

fn span_end(line: &str, at: usize) -> usize {
    let run = line[at..].bytes().take_while(|b| *b == b'`').count();
    let mut next = at + run;
    while let Some(found) = line[next..].find('`') {
        let start = next + found;
        let close = line[start..].bytes().take_while(|b| *b == b'`').count();
        if close == run {
            return start + close;
        }
        next = start + close;
    }
    at + run
}

fn token(rest: &str) -> (String, usize) {
    let mut path = String::new();
    let mut chars = rest.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c == '\\' && chars.peek().is_some_and(|(_, next)| *next == ' ') {
            chars.next();
            path.push(' ');
        } else if c.is_whitespace() {
            return (path, at);
        } else {
            path.push(c);
        }
    }
    (path, rest.len())
}
