const LCS_CELLS: usize = 4_000_000;
const CONTEXT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tools) struct Span {
    pub(in crate::tools) start: usize,
    pub(in crate::tools) len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Keep,
    Remove,
    Add,
}

fn trimmed(old: &[&str], new: &[&str]) -> (usize, usize, usize) {
    let mut head = 0;
    while head < old.len() && head < new.len() && old[head] == new[head] {
        head += 1;
    }
    let (mut old_end, mut new_end) = (old.len(), new.len());
    while old_end > head && new_end > head && old[old_end - 1] == new[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }
    (head, old_end, new_end)
}

fn steps(old: &[&str], new: &[&str]) -> Option<Vec<Step>> {
    let (head, old_end, new_end) = trimmed(old, new);
    let (middle_old, middle_new) = (&old[head..old_end], &new[head..new_end]);
    if middle_old.len().saturating_mul(middle_new.len()) > LCS_CELLS {
        return None;
    }
    let mut steps = vec![Step::Keep; head];
    steps.extend(walk(middle_old, middle_new));
    steps.extend(std::iter::repeat_n(Step::Keep, old.len() - old_end));
    Some(steps)
}

pub(in crate::tools) fn changed(old: &[&str], new: &[&str]) -> Vec<Span> {
    let Some(steps) = steps(old, new) else {
        let (head, _, new_end) = trimmed(old, new);
        return vec![Span {
            start: head,
            len: new_end - head,
        }];
    };
    let mut spans: Vec<Span> = Vec::new();
    let mut open: Option<Span> = None;
    let mut at = 0;
    for step in steps {
        match step {
            Step::Keep => {
                spans.extend(open.take());
                at += 1;
            }
            Step::Add => {
                open.get_or_insert(Span { start: at, len: 0 }).len += 1;
                at += 1;
            }
            Step::Remove => {
                open.get_or_insert(Span { start: at, len: 0 });
            }
        }
    }
    spans.extend(open);
    spans
}

fn walk(old: &[&str], new: &[&str]) -> Vec<Step> {
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
    let mut steps = Vec::with_capacity(old.len() + new.len());
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            steps.push(Step::Keep);
            i += 1;
            j += 1;
        } else if j < new.len()
            && (i == old.len() || table[i * width + j + 1] >= table[(i + 1) * width + j])
        {
            steps.push(Step::Add);
            j += 1;
        } else {
            steps.push(Step::Remove);
            i += 1;
        }
    }
    steps
}

pub(in crate::tools) fn patch(old: &[&str], new: &[&str]) -> Option<String> {
    let mut steps = steps(old, new)?;
    for run in steps.split_mut(|step| *step == Step::Keep) {
        run.sort_by_key(|step| *step == Step::Add);
    }
    let mut starts = Vec::with_capacity(steps.len() + 1);
    let (mut i, mut j) = (0, 0);
    for step in &steps {
        starts.push((i, j));
        i += usize::from(*step != Step::Add);
        j += usize::from(*step != Step::Remove);
    }
    let marks: Vec<usize> = (0..steps.len())
        .filter(|at| steps[*at] != Step::Keep)
        .collect();
    if marks.len() > crate::limits::DISPLAY_PATCH_LINES {
        return None;
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at < marks.len() {
        let first = marks[at];
        while at + 1 < marks.len() && marks[at + 1] <= marks[at] + 2 * CONTEXT {
            at += 1;
        }
        let range = first.saturating_sub(CONTEXT)..(marks[at] + 1 + CONTEXT).min(steps.len());
        out.push(hunk(&steps[range.clone()], starts[range.start], old, new));
        at += 1;
    }
    (!out.is_empty()).then(|| out.join("\n"))
}

fn hunk(steps: &[Step], (mut i, mut j): (usize, usize), old: &[&str], new: &[&str]) -> String {
    let olds = steps.iter().filter(|step| **step != Step::Add).count();
    let news = steps.iter().filter(|step| **step != Step::Remove).count();
    let mut lines = vec![format!(
        "@@ -{},{olds} +{},{news} @@",
        i + usize::from(olds > 0),
        j + usize::from(news > 0)
    )];
    for step in steps {
        let (mark, line) = match step {
            Step::Keep => (' ', old[i]),
            Step::Remove => ('-', old[i]),
            Step::Add => ('+', new[j]),
        };
        lines.push(format!("{mark}{}", line.trim_end_matches('\r')));
        i += usize::from(*step != Step::Add);
        j += usize::from(*step != Step::Remove);
    }
    lines.join("\n")
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
