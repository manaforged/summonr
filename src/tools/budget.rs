use std::borrow::{Borrow, Cow};

use crate::limits::limits;

pub(super) const OUTPUT_BUDGET: usize = 16 * 1024;
const NOTE_ROOM: usize = 1024;
pub(super) const SHAPED_BUDGET: usize = OUTPUT_BUDGET - NOTE_ROOM;
const HEAD_SHARE: usize = 6;

pub(crate) struct Digest {
    pub(crate) text: String,
    pub(crate) cut: bool,
}

pub(crate) fn within(text: &str) -> bool {
    text.len() <= OUTPUT_BUDGET
        && !text.contains("\r\n")
        && text
            .split('\n')
            .all(|line| line.len() <= limits().output_line_bytes)
}

pub(crate) fn clip_mark(limit: usize) -> String {
    format!(" [line clipped at {limit} bytes]")
}

pub(crate) fn shaped<'a, S: Borrow<str>>(
    raw: &'a str,
    select: impl FnOnce(Vec<Cow<'a, str>>) -> Vec<S>,
) -> Digest {
    let (lines, clipped) = lines(raw);
    if size(&lines) <= SHAPED_BUDGET {
        return Digest {
            text: lines.join("\n").trim_end().to_string(),
            cut: clipped,
        };
    }
    Digest {
        text: fit(&select(lines)),
        cut: true,
    }
}

fn lines(raw: &str) -> (Vec<Cow<'_, str>>, bool) {
    let limit = limits().output_line_bytes;
    let lines: Vec<_> = raw
        .lines()
        .map(|line| {
            if line.len() <= limit {
                Cow::Borrowed(line)
            } else {
                Cow::Owned(
                    crate::text::ellipsize(line, crate::text::Span::Bytes(limit))
                        + &clip_mark(limit),
                )
            }
        })
        .collect();
    let clipped = lines.iter().any(|line| matches!(line, Cow::Owned(_)));
    (lines, clipped)
}

fn size<S: Borrow<str>>(lines: &[S]) -> usize {
    lines.iter().map(|line| line.borrow().len() + 1).sum()
}

fn fit<S: Borrow<str>>(lines: &[S]) -> String {
    if size(lines) <= SHAPED_BUDGET {
        return lines.join("\n");
    }
    let head_room = SHAPED_BUDGET / HEAD_SHARE;
    let head = fits(lines.iter(), head_room);
    let tail = fits(lines[head..].iter().rev(), SHAPED_BUDGET - head_room);
    format!(
        "{}\n[{} middle lines omitted; read the full output at the path above]\n{}",
        lines[..head].join("\n"),
        lines.len() - head - tail,
        lines[lines.len() - tail..].join("\n")
    )
}

fn fits<'a, S: Borrow<str> + 'a>(lines: impl Iterator<Item = &'a S>, room: usize) -> usize {
    let mut used = 0;
    lines
        .take_while(|line| {
            used += Borrow::<str>::borrow(*line).len() + 1;
            used <= room
        })
        .count()
}
