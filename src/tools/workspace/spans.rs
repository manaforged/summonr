const LCS_CELLS: usize = 4_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tools) struct Span {
    pub(in crate::tools) start: usize,
    pub(in crate::tools) len: usize,
}

pub(in crate::tools) fn changed(old: &[&str], new: &[&str]) -> Vec<Span> {
    let mut head = 0;
    while head < old.len() && head < new.len() && old[head] == new[head] {
        head += 1;
    }
    let (mut old_end, mut new_end) = (old.len(), new.len());
    while old_end > head && new_end > head && old[old_end - 1] == new[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }
    if head == old_end && head == new_end {
        return Vec::new();
    }
    let (old, new) = (&old[head..old_end], &new[head..new_end]);
    if old.len().saturating_mul(new.len()) > LCS_CELLS {
        return vec![Span {
            start: head,
            len: new.len(),
        }];
    }
    walk(old, new)
        .into_iter()
        .map(|span| Span {
            start: span.start + head,
            ..span
        })
        .collect()
}

fn walk(old: &[&str], new: &[&str]) -> Vec<Span> {
    let width = new.len() + 1;
    let mut table = vec![0u32; (old.len() + 1) * width];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            table[i * width + j] = if old[i] == new[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut spans: Vec<Span> = Vec::new();
    let mut open: Option<Span> = None;
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            spans.extend(open.take());
            i += 1;
            j += 1;
            continue;
        }
        let span = open.get_or_insert(Span { start: j, len: 0 });
        if j < new.len()
            && (i == old.len() || table[i * width + j + 1] >= table[(i + 1) * width + j])
        {
            span.len += 1;
            j += 1;
        } else {
            i += 1;
        }
    }
    spans.extend(open);
    spans
}

pub(in crate::tools) fn summary(before: usize, total: usize, spans: &[Span]) -> String {
    if spans.is_empty() {
        return "no change".to_string();
    }
    let places = spans
        .iter()
        .map(|span| match span.len {
            0 => format!("removed after line {}", span.start),
            1 => format!("line {}", span.start + 1),
            len => format!("lines {}-{}", span.start + 1, span.start + len),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let delta = total as i64 - before as i64;
    let net = match delta {
        0 => String::new(),
        1 | -1 => format!("{delta:+} line, "),
        many => format!("{many:+} lines, "),
    };
    format!("changed {places} ({net}{total} lines now)")
}
