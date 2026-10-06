use std::io;
use std::path::{Path, PathBuf};

use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
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
    matcher: &'a RegexMatcher,
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
        let number = hit.line_number().unwrap_or(0);
        let hit = if self.tags {
            format!("{}:{number}:{}|{line}", self.rel, line_hash(&line))
        } else {
            let column = match_column(self.matcher, hit.bytes());
            format!("{}:{number}:{column}:{line}", self.rel)
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

#[derive(Default)]
struct Tally {
    hits: Vec<String>,
    matches: usize,
    match_total: usize,
    searched: usize,
    skipped: usize,
    capped: usize,
    truncated: bool,
}

pub(super) fn search(
    base: &Path,
    files: &[(PathBuf, u64)],
    matcher: &RegexMatcher,
    spec: &Spec<'_>,
) -> Result<String> {
    let Spec {
        limit,
        before,
        after,
        multiline,
        ..
    } = *spec;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .before_context(before)
        .after_context(after)
        .multi_line(multiline)
        .build();
    let mut tally = Tally::default();
    for (path, _) in files {
        if tally.matches >= limit {
            tally.truncated = true;
            break;
        }
        file(base, path, &mut searcher, matcher, spec, &mut tally);
    }
    Ok(render(spec, &tally))
}

fn file(
    base: &Path,
    path: &Path,
    searcher: &mut Searcher,
    matcher: &RegexMatcher,
    spec: &Spec<'_>,
    tally: &mut Tally,
) {
    if path.metadata().map(|meta| meta.len()).unwrap_or(0) > GREP_FILE_MAX {
        tally.skipped += 1;
        return;
    }
    let rel = relative(base, path);
    let mut collect = Collect {
        rel: &rel,
        matcher,
        out: Vec::new(),
        matches: 0,
        cap: GREP_FILE_HITS,
        mode: spec.mode,
        tags: spec.tags,
        over: false,
        matched: false,
        binary: false,
    };
    tally.searched += 1;
    if searcher.search_path(matcher, path, &mut collect).is_err()
        || (collect.binary && !collect.matched)
    {
        tally.skipped += 1;
        return;
    }
    if collect.over {
        tally.capped += 1;
    }
    match spec.mode {
        Mode::Files => {
            if collect.matched {
                tally.hits.push(rel);
                tally.matches += 1;
            }
        }
        Mode::Count => {
            if collect.matches > 0 {
                tally.hits.push(format!("{rel}:{}", collect.matches));
                tally.matches += 1;
                tally.match_total += collect.matches;
            }
        }
        Mode::Content => drain(collect.out, spec.limit, tally),
    }
}

fn drain(rows: Vec<Row>, limit: usize, tally: &mut Tally) {
    let mut pending_break = false;
    for row in rows {
        match row {
            Row::Break => {
                pending_break = !tally.hits.is_empty();
            }
            Row::Ctx(line) => {
                if pending_break {
                    tally.hits.push("--".to_string());
                    pending_break = false;
                }
                tally.hits.push(line);
            }
            Row::Hit(line) => {
                if tally.matches >= limit {
                    tally.truncated = true;
                    break;
                }
                tally.matches += 1;
                if pending_break {
                    tally.hits.push("--".to_string());
                    pending_break = false;
                }
                tally.hits.push(line);
            }
        }
    }
}

fn render(spec: &Spec<'_>, tally: &Tally) -> String {
    if tally.hits.is_empty() {
        let mut out = format!(
            "no matches for `{}` ({} files searched",
            spec.pattern, tally.searched
        );
        if tally.skipped > 0 {
            out.push_str(&format!(
                ", {} skipped as binary, over 1 MiB, or unreadable",
                tally.skipped
            ));
        }
        out.push(')');
        return out;
    }
    let mut out = match spec.mode {
        Mode::Files => format!(
            "{} with `{}` ({} searched)",
            crate::text::plural(tally.matches, "file"),
            spec.pattern,
            tally.searched
        ),
        Mode::Count => format!(
            "{} in {} ({} searched)",
            crate::text::plural(tally.match_total, "match"),
            crate::text::plural(tally.matches, "file"),
            tally.searched
        ),
        Mode::Content => format!(
            "{} for `{}` ({} files searched)",
            crate::text::plural(tally.matches, "match"),
            spec.pattern,
            tally.searched
        ),
    };
    if tally.truncated {
        out.push_str(&format!(
            "; limit {} reached: narrow with path, glob, or a more specific pattern",
            spec.limit
        ));
    }
    if tally.capped > 0 {
        out.push_str(&format!(
            "; {} had more than {GREP_FILE_HITS} matching lines",
            crate::text::plural(tally.capped, "file"),
        ));
    }
    if tally.skipped > 0 {
        out.push_str(&format!(
            "; {} files skipped as binary, over 1 MiB, or unreadable",
            tally.skipped
        ));
    }
    for hit in &tally.hits {
        out.push('\n');
        out.push_str(hit);
    }
    out
}

fn match_column(matcher: &RegexMatcher, bytes: &[u8]) -> usize {
    let line = bytes
        .strip_suffix(b"\n")
        .map_or(bytes, |line| line.strip_suffix(b"\r").unwrap_or(line));
    let Ok(Some(found)) = matcher.find(line) else {
        return 1;
    };
    String::from_utf8_lossy(&line[..found.start()])
        .chars()
        .map(char::len_utf16)
        .sum::<usize>()
        + 1
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
