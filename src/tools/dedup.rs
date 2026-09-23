use super::{ToolOutput, ToolRegistry, object_bool};

pub(super) type Ranges = std::collections::HashMap<String, ((u64, u64), Vec<(usize, usize)>)>;

impl ToolRegistry {
    pub(crate) fn forget_reads(&self) {
        self.read_ranges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    pub(super) fn dedup_read(&self, arguments: &str, out: ToolOutput) -> ToolOutput {
        let Some((path, served)) = served_range(arguments, &out.text) else {
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
        let mut cursor = served.0;
        for &(a, b) in &entry.1 {
            if a <= cursor {
                cursor = cursor.max(b + 1);
            }
            if cursor > served.1 {
                break;
            }
        }
        if cursor > served.1 {
            return ToolOutput {
                text: format!(
                    "{path} lines {}-{} already shown by earlier reads; file unchanged",
                    served.0, served.1
                ),
                ..out
            };
        }
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
        out
    }
}

fn served_range(arguments: &str, text: &str) -> Option<(String, (usize, usize))> {
    let args: serde_json::Value = serde_json::from_str(arguments).ok()?;
    if args
        .get("op")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|op| op != "file")
        || args.get("pattern").is_some()
        || args.get("paths").is_some()
        || object_bool(&args, "tags")
    {
        return None;
    }
    let path = args.get("path")?.as_str()?.to_string();
    let seg = text.lines().next()?.rsplit_once("  lines ")?.1;
    let (range, _) = seg.split_once('/')?;
    let (a, b) = range.split_once('-')?;
    Some((path, (a.parse().ok()?, b.parse().ok()?)))
}
