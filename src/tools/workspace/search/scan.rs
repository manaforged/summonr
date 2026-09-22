use std::io;
use std::path::{Path, PathBuf};

use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch};

use crate::error::Result;
use crate::hashline::line_hash;

use super::{GREP_FILE_HITS, GREP_FILE_MAX, GREP_LINE_MAX, Mode, Spec, relative};
enum Row {
    Hit(String),
    Ctx(String),
    Break,
}

struct Collect<'a> {
    rel: &'a str,
    out: Vec<Row>,
    matches: usize,
    cap: usize,
    mode: Mode,
    tags: bool,
    over: bool,
    matched: bool,
    binary: bool,
}

impl Sink for Collect<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, hit: &SinkMatch<'_>) -> io::Result<bool> {
        self.matched = true;
        match self.mode {
            Mode::Files => return Ok(false),
            Mode::Count => {
                self.matches += 1;
                return Ok(true);
            }
            Mode::Content => {}
        }
        if self.matches >= self.cap {
            self.over = true;
            return Ok(false);
        }
        self.matches += 1;
        let line = hit_line(hit.bytes());
        let hit = if self.tags {
            format!(
                "{}:{}:{}|{}",
                self.rel,
                hit.line_number().unwrap_or(0),
                line_hash(&line),
                line
            )
        } else {
            format!("{}:{}:{}", self.rel, hit.line_number().unwrap_or(0), line)
        };
        self.out.push(Row::Hit(hit));
        Ok(true)
    }

    fn context(&mut self, _searcher: &Searcher, ctx: &SinkContext<'_>) -> io::Result<bool> {
        if self.mode != Mode::Content {
            return Ok(true);
        }
        self.out.push(Row::Ctx(format!(
            "{}-{}-{}",
            self.rel,
            ctx.line_number().unwrap_or(0),
            hit_line(ctx.bytes())
        )));
        Ok(true)
    }

    fn context_break(&mut self, _searcher: &Searcher) -> io::Result<bool> {
        if self.mode == Mode::Content
            && !self.out.is_empty()
            && !matches!(self.out.last(), Some(Row::Break))
        {
            self.out.push(Row::Break);
        }
        Ok(true)
    }

    fn binary_data(&mut self, _searcher: &Searcher, _offset: u64) -> io::Result<bool> {
        self.binary = true;
        Ok(false)
    }
}

pub(super) fn search(
    base: &Path,
    files: &[(PathBuf, u64)],
    matcher: &grep_regex::RegexMatcher,
    spec: &Spec<'_>,
) -> Result<String> {
    let Spec {
        pattern,
        limit,
        before,
        after,
        mode,
        multiline,
        tags,
    } = *spec;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .before_context(before)
        .after_context(after)
        .multi_line(multiline)
        .build();
    let mut hits: Vec<String> = Vec::new();
    let mut match_count = 0_usize;
    let mut match_total = 0_usize;
    let mut searched = 0_usize;
    let mut skipped = 0_usize;
    let mut capped = 0_usize;
    let mut truncated = false;
    for (path, _) in files {
        if match_count >= limit {
            truncated = true;
            break;
        }
        if path.metadata().map(|meta| meta.len()).unwrap_or(0) > GREP_FILE_MAX {
            skipped += 1;
            continue;
        }
        let rel = relative(base, path);
        let mut collect = Collect {
            rel: &rel,
            out: Vec::new(),
            matches: 0,
            cap: GREP_FILE_HITS,
            mode,
            tags,
            over: false,
            matched: false,
            binary: false,
        };
        searched += 1;
        if searcher.search_path(matcher, path, &mut collect).is_err() {
            skipped += 1;
            continue;
        }
        if collect.binary && !collect.matched {
            skipped += 1;
            continue;
        }
        if collect.over {
            capped += 1;
        }
        match mode {
            Mode::Files => {
                if collect.matched {
                    hits.push(rel);
                    match_count += 1;
                }
                continue;
            }
            Mode::Count => {
                if collect.matches > 0 {
                    hits.push(format!("{rel}:{}", collect.matches));
                    match_count += 1;
                    match_total += collect.matches;
                }
                continue;
            }
            Mode::Content => {}
        }
        let mut pending_break = false;
        for row in collect.out {
            match row {
                Row::Break => {
                    pending_break = !hits.is_empty();
                }
                Row::Ctx(line) => {
                    if pending_break {
                        hits.push("--".to_string());
                        pending_break = false;
                    }
                    hits.push(line);
                }
                Row::Hit(line) => {
                    if match_count >= limit {
                        truncated = true;
                        break;
                    }
                    match_count += 1;
                    if pending_break {
                        hits.push("--".to_string());
                        pending_break = false;
                    }
                    hits.push(line);
                }
            }
        }
    }
    if hits.is_empty() {
        let mut out = format!("no matches for `{pattern}` ({searched} files searched");
        if skipped > 0 {
            out.push_str(&format!(
                ", {skipped} skipped as binary, over 1 MiB, or unreadable"
            ));
        }
        out.push(')');
        return Ok(out);
    }
    let mut out = match mode {
        Mode::Files => format!(
            "{} with `{pattern}` ({searched} searched)",
            plural(match_count, "file"),
        ),
        Mode::Count => format!(
            "{} in {} ({searched} searched)",
            plural(match_total, "match"),
            plural(match_count, "file"),
        ),
        Mode::Content => format!(
            "{} for `{pattern}` ({searched} files searched)",
            plural(match_count, "match"),
        ),
    };
    if truncated {
        out.push_str(&format!(
            "; limit {limit} reached — narrow with path, glob, or a more specific pattern"
        ));
    }
    if capped > 0 {
        out.push_str(&format!(
            "; {} had more than {GREP_FILE_HITS} matching lines",
            plural(capped, "file"),
        ));
    }
    if skipped > 0 {
        out.push_str(&format!(
            "; {skipped} files skipped as binary, over 1 MiB, or unreadable"
        ));
    }
    for hit in hits {
        out.push('\n');
        out.push_str(&hit);
    }
    Ok(out)
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("{count} {word}")
    } else if word.ends_with(['s', 'x']) || word.ends_with("ch") || word.ends_with("sh") {
        format!("{count} {word}es")
    } else {
        format!("{count} {word}s")
    }
}

fn hit_line(bytes: &[u8]) -> String {
    let line = String::from_utf8_lossy(bytes);
    let line = line.trim_end_matches(['\n', '\r']);
    if line.len() <= GREP_LINE_MAX {
        return line.to_string();
    }
    let mut end = GREP_LINE_MAX;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [line clipped at {GREP_LINE_MAX} bytes]", &line[..end])
}
