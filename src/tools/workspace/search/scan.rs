use std::io;
use std::path::{Path, PathBuf};

use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch};

use crate::hashline::line_hash;

use super::walk::Shared;
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
        let number = hit.line_number().unwrap_or(0);
        if self.tags {
            for (at, raw) in (number..).zip(hit.lines()) {
                let raw = String::from_utf8_lossy(raw);
                let raw = raw.trim_end_matches(['\n', '\r']);
                self.out.push(Row::Hit(format!(
                    "{}:{at}:{}|{}",
                    self.rel,
                    line_hash(raw),
                    hit_line(raw)
                )));
            }
            return Ok(true);
        }
        let column = match_column(self.matcher, hit.bytes());
        let line = hit_line(&String::from_utf8_lossy(hit.bytes()));
        self.out
            .push(Row::Hit(format!("{}:{number}:{column}:{line}", self.rel)));
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
            hit_line(&String::from_utf8_lossy(ctx.bytes()))
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
    pub(super) rows: Vec<Row>,
    pub(super) matches: usize,
    pub(super) over: bool,
}

pub(super) struct Found {
    pub(super) path: PathBuf,
    pub(super) modified: u64,
    pub(super) hits: Hits,
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

fn hit_line(line: &str) -> String {
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
