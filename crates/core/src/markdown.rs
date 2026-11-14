use std::ops::Range;

const FENCE_CHARS: [char; 2] = ['`', '~'];
const FENCE_MIN: usize = 3;
const FENCE_INDENT: usize = 3;
const TICK: u8 = b'`';

/// The fenced code block open at the current line, if any.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fence(Option<(char, usize)>);

impl Fence {
    /// Steps past `line`; true when the line opens, closes, or sits inside a fenced code block.
    pub fn code(&mut self, line: &str) -> bool {
        let mark = mark(line);
        let was = self.0;
        self.0 = match (was, mark) {
            (None, mark) => mark.map(|(open, width, _)| (open, width)),
            (Some((open, width)), Some((close, run, true))) if close == open && run >= width => {
                None
            }
            (open @ Some(_), Some(_) | None) => open,
        };
        was.is_some() || mark.is_some()
    }

    /// Whether a fenced code block is open.
    pub fn open(&self) -> bool {
        self.0.is_some()
    }
}

fn mark(line: &str) -> Option<(char, usize, bool)> {
    let body = line.trim_start_matches(' ');
    if line.len() - body.len() > FENCE_INDENT {
        return None;
    }
    let open = body.chars().next().filter(|c| FENCE_CHARS.contains(c))?;
    let width = body.chars().take_while(|c| *c == open).count();
    let bare = body[width..].trim().is_empty();
    (width >= FENCE_MIN).then_some((open, width, bare))
}

/// The byte after the backtick code span that opens at `at` in `line`, or after its opening run when no run of the same length closes it.
pub fn span_end(line: &str, at: usize) -> usize {
    let run = line[at..].bytes().take_while(|b| *b == TICK).count();
    let mut next = at + run;
    while let Some(found) = line[next..].bytes().position(|b| b == TICK) {
        let start = next + found;
        let close = line[start..].bytes().take_while(|b| *b == TICK).count();
        if close == run {
            return start + close;
        }
        next = start + close;
    }
    at + run
}

/// One `@path` mention: where it sits in its line, the path, and whether it was written as a quoted string.
pub struct Mention {
    /// The bytes of the line from the `@` to the end of the path.
    pub span: Range<usize>,
    /// The path, with `\ ` escapes and JSON quotes undone.
    pub path: String,
    /// Whether the path was written in double quotes.
    pub quoted: bool,
}

/// Every `@path` mention in `text` outside fenced code blocks and code spans.
pub fn mentions(text: &str) -> Vec<Mention> {
    let mut fence = Fence::default();
    text.lines()
        .filter(|line| !fence.code(line))
        .flat_map(targets)
        .collect()
}

/// Every `@path` mention in one `line` outside its code spans.
pub fn targets(line: &str) -> Vec<Mention> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'`' {
            at = span_end(line, at);
        } else if bytes[at] == b'@' && (at == 0 || bytes[at - 1].is_ascii_whitespace()) {
            let rest = &line[at + 1..];
            let (path, len) = quoted(rest).unwrap_or_else(|| token(rest));
            if !path.is_empty() {
                out.push(Mention {
                    span: at..at + 1 + len,
                    path,
                    quoted: rest.starts_with('"'),
                });
            }
            at += 1 + len;
        } else {
            at += 1;
        }
    }
    out
}

fn quoted(rest: &str) -> Option<(String, usize)> {
    rest.strip_prefix('"')?;
    let mut parsed = serde_json::Deserializer::from_str(rest).into_iter::<String>();
    match parsed.next() {
        Some(Ok(path)) => Some((path, parsed.byte_offset())),
        Some(Err(_)) | None => None,
    }
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
