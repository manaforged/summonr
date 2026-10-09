use std::io;
use std::path::{Path, PathBuf};

use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch};

use crate::hashline::line_hash;

use super::walk::{Report, Shared};
use super::{GREP_FILE_HITS, GREP_FILE_MAX, GREP_LINE_MAX, Mode, Query, relative};

pub(super) enum Row {
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
pub(super) struct Hits {
    rows: Vec<Row>,
    pub(super) matches: usize,
    pub(super) over: bool,
}

pub(super) struct Found {
    pub(super) path: PathBuf,
    pub(super) modified: u64,
    pub(super) hits: Hits,
}

#[derive(Default)]
struct Tally {
    hits: Vec<String>,
    matches: usize,
    truncated: bool,
}

pub(super) fn searcher(query: &Query) -> Searcher {
    SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .before_context(query.before)
        .after_context(query.after)
        .multi_line(query.multiline)
        .build()
}

pub(super) fn file(
    query: &Query,
    shared: &Shared,
    searcher: &mut Searcher,
    matcher: &RegexMatcher,
    path: &Path,
    size: u64,
) -> Option<Hits> {
    use std::sync::atomic::Ordering::Relaxed;
    if size > GREP_FILE_MAX {
        shared.skipped.fetch_add(1, Relaxed);
        return None;
    }
    let rel = relative(&query.shown, path);
    let mut collect = Collect {
        rel: &rel,
        matcher,
        out: Vec::new(),
        matches: 0,
        cap: GREP_FILE_HITS,
        mode: query.mode,
        tags: query.tags,
        over: false,
        matched: false,
        binary: false,
    };
    shared.searched.fetch_add(1, Relaxed);
    if open(path)
        .and_then(|file| searcher.search_file(matcher, &file, &mut collect))
        .is_err()
        || (collect.binary && !collect.matched)
    {
        shared.skipped.fetch_add(1, Relaxed);
        return None;
    }
    collect.matched.then_some(Hits {
        rows: collect.out,
        matches: collect.matches,
        over: collect.over,
    })
}

#[cfg(unix)]
fn open(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open(path: &Path) -> io::Result<std::fs::File> {
    std::fs::File::open(path)
}

pub(super) fn render(query: &Query, mut report: Report) -> String {
    if query.matcher.is_none() {
        return file_list(query, &report);
    }
    let mut tally = Tally::default();
    let mut seen = 0;
    for found in std::mem::take(&mut report.found) {
        if tally.matches >= query.limit {
            tally.truncated = true;
            break;
        }
        seen += 1;
        let rel = relative(&query.shown, &found.path);
        match query.mode {
            Mode::Files => {
                tally.hits.push(rel);
                tally.matches += 1;
            }
            Mode::Count => {
                tally.hits.push(format!("{rel}:{}", found.hits.matches));
                tally.matches += 1;
            }
            Mode::Content => drain(found.hits.rows, query.limit, &mut tally),
        }
    }
    tally.truncated |= seen < report.files;
    header(query, &report, &tally)
}

fn header(query: &Query, report: &Report, tally: &Tally) -> String {
    let pattern = &query.pattern;
    if tally.hits.is_empty() {
        let mut out = format!(
            "no matches for `{pattern}` ({} files searched",
            report.searched
        );
        if report.skipped > 0 {
            out.push_str(&format!(
                ", {} skipped as binary, over 1 MiB, or unreadable",
                report.skipped
            ));
        }
        out.push(')');
        if !query.glob {
            out.push_str("; to match file names, pass glob alone");
        }
        out.push_str(&skipped_note(query));
        out.push_str(&stopped_note(report.stopped));
        return out;
    }
    let mut out = match query.mode {
        Mode::Files => format!(
            "{} with `{pattern}`, newest first ({} searched)",
            crate::text::plural(report.files, "file"),
            report.searched
        ),
        Mode::Count => format!(
            "{} in {} ({} searched)",
            crate::text::plural(report.match_total, "match"),
            crate::text::plural(report.files, "file"),
            report.searched
        ),
        Mode::Content => format!(
            "{} for `{pattern}`, newest file first ({} files searched)",
            crate::text::plural(tally.matches, "match"),
            report.searched
        ),
    };
    if tally.truncated {
        out.push_str(&format!(
            "; limit {} reached: narrow with path, glob, or a more specific pattern",
            query.limit
        ));
    }
    if report.capped > 0 {
        out.push_str(&format!(
            "; {} had more than {GREP_FILE_HITS} matching lines",
            crate::text::plural(report.capped, "file"),
        ));
    }
    if report.skipped > 0 {
        out.push_str(&format!(
            "; {} files skipped as binary, over 1 MiB, or unreadable",
            report.skipped
        ));
    }
    out.push_str(&stopped_note(report.stopped));
    for hit in &tally.hits {
        out.push('\n');
        out.push_str(hit);
    }
    out
}

fn file_list(query: &Query, report: &Report) -> String {
    if report.found.is_empty() {
        return format!(
            "no files match{}{}",
            skipped_note(query),
            stopped_note(report.stopped)
        );
    }
    let shown = report.found.len().min(query.limit);
    let mut out = format!(
        "{}, newest first",
        crate::text::plural(report.files, "file")
    );
    if report.files > shown {
        out.push_str(&format!("; showing {shown} most recently modified"));
    }
    out.push_str(&stopped_note(report.stopped));
    for found in report.found.iter().take(shown) {
        out.push('\n');
        out.push_str(&relative(&query.shown, &found.path));
    }
    out
}

fn skipped_note(query: &Query) -> String {
    let folders = crate::SKIP_DIRS.join(", ");
    if query.no_ignore {
        return format!("; {folders} folders skipped");
    }
    format!("; {folders} folders and gitignored files skipped (no_ignore true includes the files)")
}

fn stopped_note(stopped: bool) -> String {
    if !stopped {
        return String::new();
    }
    format!(
        "; stopped after {}s: narrow path, glob, or depth for the rest",
        crate::limits::limits().search_secs
    )
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
