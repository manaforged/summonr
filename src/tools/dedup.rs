use super::{ToolOutput, ToolRegistry, object_bool};

pub(super) type Ranges = std::collections::HashMap<String, ((u64, u64), Vec<(usize, usize)>)>;

impl ToolRegistry {
    pub(crate) fn forget_reads(&self) {
        self.read_ranges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    pub(super) fn dedup_read(&self, args: &serde_json::Value, out: ToolOutput) -> ToolOutput {
        let Some((path, served)) = served_range(args, &out.text) else {
            return out;
        };
        let stamp = std::fs::metadata(self.workspace.join(&path))
            .map(|m| {
                (
                    m.modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or(0),
                    m.len(),
                )
            })
            .unwrap_or((0, 0));
        let Ok(mut seen) = self.read_ranges.lock() else {
            return out;
        };
        let entry = seen.entry(path.clone()).or_insert((stamp, Vec::new()));
        if entry.0 != stamp {
            *entry = (stamp, Vec::new());
        }
        let text = match clip(&out.text, &entry.1) {
            Clip::Repeat => {
                return ToolOutput {
                    text: format!(
                        "{path} lines {}-{} already shown by earlier reads; file unchanged",
                        served.0, served.1
                    ),
                    ..out
                };
            }
            Clip::Partial(text) => text,
            Clip::Fresh => out.text,
        };
        entry.1.push(served);
        entry.1.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(entry.1.len());
        for &(a, b) in entry.1.iter() {
            if let Some(last) = merged.last_mut()
                && a <= last.1 + 1
            {
                last.1 = last.1.max(b);
            } else {
                merged.push((a, b));
            }
        }
        *entry = (entry.0, merged);
        ToolOutput { text, ..out }
    }
}

enum Clip {
    Fresh,
    Repeat,
    Partial(String),
}

fn clip(text: &str, shown: &[(usize, usize)]) -> Clip {
    if shown.is_empty() {
        return Clip::Fresh;
    }
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Clip::Fresh;
    };
    let Some((path, rest)) = header.split_once("  lines ") else {
        return Clip::Fresh;
    };
    let Some((_, total_rest)) = rest.split_once('/') else {
        return Clip::Fresh;
    };
    let total = total_rest.split_whitespace().next().unwrap_or(total_rest);
    let mut kept = Vec::new();
    let mut skipped = 0usize;
    let mut tail = Vec::new();
    let mut body = true;
    for line in lines {
        if body {
            if let Some(number) = line_no(line) {
                if covers(shown, number) {
                    skipped += 1;
                } else {
                    kept.push(line);
                }
                continue;
            }
            body = false;
        }
        if !line.is_empty() {
            tail.push(line);
        }
    }
    if skipped == 0 {
        return Clip::Fresh;
    }
    if kept.is_empty() {
        return Clip::Repeat;
    }
    let Some(first) = line_no(kept[0]) else {
        return Clip::Fresh;
    };
    let Some(last) = kept.last().copied().and_then(line_no) else {
        return Clip::Fresh;
    };
    let mut out = format!("{path}  lines {first}-{last}/{total}  [{skipped} already shown]");
    for line in kept {
        out.push('\n');
        out.push_str(line);
    }
    for line in tail {
        out.push('\n');
        out.push_str(line);
    }
    Clip::Partial(out)
}

fn line_no(line: &str) -> Option<usize> {
    let (n, _) = line.split_once(':')?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

fn covers(shown: &[(usize, usize)], number: usize) -> bool {
    shown.iter().any(|&(a, b)| number >= a && number <= b)
}

fn served_range(args: &serde_json::Value, text: &str) -> Option<(String, (usize, usize))> {
    if args
        .get("op")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|op| op != "file")
        || args.get("pattern").is_some()
        || args.get("paths").is_some()
        || object_bool(args, "tags")
    {
        return None;
    }
    let path = args.get("path")?.as_str()?.to_string();
    Some((path, read_span(text)?))
}

pub(crate) fn read_span(text: &str) -> Option<(usize, usize)> {
    let seg = text.lines().next()?.rsplit_once("  lines ")?.1;
    let (range, _) = seg.split_once('/')?;
    let (a, b) = range.split_once('-')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}
