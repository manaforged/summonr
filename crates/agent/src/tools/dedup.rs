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
        let text = match omit_shown(&out.text, &entry.1) {
            Omitted::Repeat => {
                return ToolOutput {
                    text: format!(
                        "{path} lines {}-{} already shown by earlier reads; file unchanged",
                        served.0, served.1
                    ),
                    ..out
                };
            }
            Omitted::Partial(text) => text,
            Omitted::Fresh => out.text,
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

enum Omitted {
    Fresh,
    Repeat,
    Partial(String),
}

fn omit_shown(text: &str, shown: &[(usize, usize)]) -> Omitted {
    if shown.is_empty() {
        return Omitted::Fresh;
    }
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Omitted::Fresh;
    };
    let Some((path, rest)) = header.split_once("  lines ") else {
        return Omitted::Fresh;
    };
    let Some((_, total_rest)) = rest.split_once('/') else {
        return Omitted::Fresh;
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
        return Omitted::Fresh;
    }
    if kept.is_empty() {
        return Omitted::Repeat;
    }
    let Some(first) = line_no(kept[0]) else {
        return Omitted::Fresh;
    };
    let Some(last) = kept.last().copied().and_then(line_no) else {
        return Omitted::Fresh;
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
    Omitted::Partial(out)
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

pub fn read_span(text: &str) -> Option<(usize, usize)> {
    let seg = text.lines().next()?.rsplit_once("  lines ")?.1;
    let (range, _) = seg.split_once('/')?;
    let (a, b) = range.split_once('-')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// Drops the body of a read result whose lines can be read again from `workspace`; true when the result is compact.
pub fn compact_read(arg: &str, output: &mut String, workspace: &std::path::Path) -> bool {
    let (header, body) = output.split_once('\n').unwrap_or((output.as_str(), ""));
    if !header.starts_with(arg)
        || header.contains("already shown")
        || body.contains("[truncated ")
        || body.contains("[invalid UTF-8 replaced]")
        || read_span(header).is_none()
        || !read_path(workspace, header).is_some_and(|path| path.is_file())
    {
        return false;
    }
    let keep = header.len();
    if keep < output.len() {
        output.truncate(keep);
        output.shrink_to_fit();
    }
    true
}

/// The read result a compact `header` stands for, read again from `workspace`.
pub fn rehydrate_read(workspace: &std::path::Path, header: &str) -> Option<String> {
    use std::fmt::Write as _;
    use std::io::BufRead as _;
    let (from, to) = read_span(header)?;
    if from == 0 || to < from {
        return None;
    }
    let path = read_path(workspace, header)?;
    let reader = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let mut out = String::with_capacity(header.len() + (to - from + 1) * 64);
    out.push_str(header);
    let mut kept = 0_usize;
    for (index, line) in reader.lines().enumerate() {
        let number = index + 1;
        if number > to {
            break;
        }
        let line = line.ok()?;
        if number >= from {
            write!(out, "\n{number}:{line}").ok()?;
            kept += 1;
        }
    }
    (kept > 0).then_some(out)
}

fn read_path(workspace: &std::path::Path, header: &str) -> Option<std::path::PathBuf> {
    if workspace.as_os_str().is_empty() {
        return None;
    }
    let rel = header.split("  lines ").next()?.trim();
    if rel.is_empty() {
        return None;
    }
    let path = std::path::Path::new(rel);
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(workspace.join(path))
}
