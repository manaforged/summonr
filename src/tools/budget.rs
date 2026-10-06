use std::borrow::{Borrow, Cow};

const OUTPUT_BUDGET: usize = 50 * 1024;
const NOTE_ROOM: usize = 1024;
const SHAPED_BUDGET: usize = OUTPUT_BUDGET - NOTE_ROOM;
const HEAD_SHARE: usize = 6;
const LINE_MAX: usize = 8 * 1024;
const CLIP_MARK: &str = " [line clipped]";

pub(crate) struct Digest {
    pub(crate) text: String,
    pub(crate) cut: bool,
}

pub(crate) fn within(text: &str) -> bool {
    text.len() <= OUTPUT_BUDGET
        && !text.contains("\r\n")
        && text.split('\n').all(|line| line.len() <= LINE_MAX)
}

pub(crate) fn plain(raw: &str) -> Digest {
    shaped(raw, |lines| lines)
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
    let mut clipped = false;
    let lines = raw
        .lines()
        .map(|line| {
            if line.len() > LINE_MAX {
                clipped = true;
                let keep = line.floor_char_boundary(LINE_MAX - CLIP_MARK.len());
                Cow::Owned(format!("{}{CLIP_MARK}", &line[..keep]))
            } else {
                Cow::Borrowed(line)
            }
        })
        .collect();
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
