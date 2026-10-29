use super::*;

pub(super) fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(String::from)
        .map_err(|()| Error::Tool(format!("cannot convert {} to a file URI", path.display())))
}

const SEVERITY_ERROR: u64 = 1;

fn entries(value: &Value) -> Vec<Value> {
    value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| value.as_array().cloned())
        .unwrap_or_default()
}

fn severity(item: &Value) -> u64 {
    item.get("severity")
        .and_then(Value::as_u64)
        .unwrap_or(SEVERITY_ERROR)
}

pub(super) fn diagnostic_items(value: &Value) -> Vec<String> {
    entries(value)
        .iter()
        .filter_map(format_diagnostic)
        .collect()
}

pub(super) fn has_error(value: &Value) -> bool {
    entries(value)
        .iter()
        .any(|item| severity(item) == SEVERITY_ERROR && format_diagnostic(item).is_some())
}

pub(super) fn format_diagnostic(item: &Value) -> Option<String> {
    let message = item.get("message")?.as_str()?.trim();
    if message.is_empty() {
        return None;
    }
    let severity = match severity(item) {
        SEVERITY_ERROR => "error",
        2 => "warning",
        3 => "info",
        _ => "hint",
    };
    let line = item
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let col = item
        .pointer("/range/start/character")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    Some(format!("{severity} {line}:{col}: {message}"))
}

pub(super) fn locations(value: &Value) -> Vec<String> {
    let list = if let Some(array) = value.as_array() {
        array.clone()
    } else if value.is_object() {
        vec![value.clone()]
    } else {
        return Vec::new();
    };
    list.iter().filter_map(format_location).collect()
}

pub(super) struct TextEdit {
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
    new_text: String,
}

pub(super) enum Change {
    Text { uri: String, edits: Vec<TextEdit> },
    Create { uri: String },
    Rename { from: String, to: String },
    Delete { uri: String },
}

pub(super) fn collect_changes(edit: &Value) -> Result<Vec<Change>> {
    let mut ops = Vec::new();
    if let Some(changes) = edit.get("changes").and_then(Value::as_object) {
        for (uri, value) in changes {
            let edits = parse_edits(value)?;
            if !edits.is_empty() {
                ops.push(Change::Text {
                    uri: uri.clone(),
                    edits,
                });
            }
        }
    }
    if let Some(docs) = edit.get("documentChanges").and_then(Value::as_array) {
        for item in docs {
            if let Some(change) = document_change(item)? {
                ops.push(change);
            }
        }
    }
    Ok(ops)
}

fn document_change(item: &Value) -> Result<Option<Change>> {
    let change = match item.get("kind").and_then(Value::as_str) {
        Some("create") => Change::Create {
            uri: required(item, "/uri", "lsp create is missing a uri")?,
        },
        Some("rename") => Change::Rename {
            from: required(item, "/oldUri", "lsp rename is missing oldUri")?,
            to: required(item, "/newUri", "lsp rename is missing newUri")?,
        },
        Some("delete") => Change::Delete {
            uri: required(item, "/uri", "lsp delete is missing a uri")?,
        },
        Some(other) => {
            return Err(Error::Tool(format!(
                "lsp rename includes unsupported file operation `{other}`"
            )));
        }
        None => {
            let uri = required(
                item,
                "/textDocument/uri",
                "lsp rename is missing a document uri",
            )?;
            let edits = item
                .get("edits")
                .map(parse_edits)
                .transpose()?
                .unwrap_or_default();
            if edits.is_empty() {
                return Ok(None);
            }
            Change::Text { uri, edits }
        }
    };
    Ok(Some(change))
}

fn required(item: &Value, pointer: &str, missing: &str) -> Result<String> {
    item.pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Tool(missing.to_string()))
}

pub(super) fn parse_edits(value: &Value) -> Result<Vec<TextEdit>> {
    let Some(items) = value.as_array() else {
        return Ok(Vec::new());
    };
    items.iter().map(parse_edit).collect()
}

pub(super) fn parse_edit(item: &Value) -> Result<TextEdit> {
    let new_text = item
        .get("newText")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Tool("lsp edit is missing newText".to_string()))?;
    let start_line = item
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Tool("lsp edit is missing a range".to_string()))?;
    let start_col = item
        .pointer("/range/start/character")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Tool("lsp edit is missing a range".to_string()))?;
    let end_line = item
        .pointer("/range/end/line")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Tool("lsp edit is missing a range".to_string()))?;
    let end_col = item
        .pointer("/range/end/character")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Tool("lsp edit is missing a range".to_string()))?;
    Ok(TextEdit {
        start_line: start_line as usize,
        start_col: start_col as usize,
        end_line: end_line as usize,
        end_col: end_col as usize,
        new_text: new_text.to_string(),
    })
}

pub(super) fn apply_text_edits(text: &str, edits: &[TextEdit]) -> Result<String> {
    let mut ordered: Vec<&TextEdit> = edits.iter().collect();
    ordered.sort_by_key(|edit| (edit.start_line, edit.start_col, edit.end_line, edit.end_col));
    for pair in ordered.windows(2) {
        let left = pair[0];
        let right = pair[1];
        if (left.end_line, left.end_col) > (right.start_line, right.start_col) {
            return Err(Error::Tool(
                "lsp rename edits overlap; apply them one at a time".to_string(),
            ));
        }
    }
    let rows: Vec<&str> = text.split('\n').collect();
    let mut groups: Vec<Vec<&TextEdit>> = Vec::new();
    let mut reach: Option<usize> = None;
    for edit in ordered {
        match groups.last_mut() {
            Some(group) if reach.is_some_and(|line| line >= edit.start_line) => group.push(edit),
            _ => groups.push(vec![edit]),
        }
        reach = Some(reach.map_or(edit.end_line, |line| line.max(edit.end_line)));
    }
    let patches = groups
        .iter()
        .map(|group| line_patch(&rows, group))
        .collect::<Result<Vec<_>>>()?;
    Ok(edit::apply_patches(text, patches)?.0)
}

fn line_patch(rows: &[&str], group: &[&TextEdit]) -> Result<edit::Patch> {
    if group
        .iter()
        .any(|edit| (edit.start_line, edit.start_col) > (edit.end_line, edit.end_col))
    {
        return Err(Error::Tool("lsp edit range is inverted".to_string()));
    }
    let first = group[0].start_line;
    let last = group
        .iter()
        .map(|edit| edit.end_line)
        .max()
        .unwrap_or(first);
    let span = rows
        .get(first..=last)
        .ok_or_else(|| Error::Tool("lsp edit line is past the end of the file".to_string()))?
        .join("\n");
    let byte = |line: usize, character: usize| -> Result<usize> {
        // complexity: bounded by the lines one rename group spans
        let before: usize = rows[first..line].iter().map(|row| row.len() + 1).sum();
        Ok(before + utf16_byte(rows[line], character)?)
    };
    let mut out = String::with_capacity(span.len());
    let mut kept = 0;
    for edit in group {
        let start = byte(edit.start_line, edit.start_col)?;
        let end = byte(edit.end_line, edit.end_col)?;
        out.push_str(&span[kept..start]);
        out.push_str(&edit.new_text);
        kept = end;
    }
    out.push_str(&span[kept..]);
    Ok(edit::Patch {
        start: first,
        end: last + 1,
        anchor_start: first,
        anchor_end: last + 1,
        replacement: out.split('\n').map(str::to_string).collect(),
        relaxed: false,
    })
}

fn utf16_byte(row: &str, character: usize) -> Result<usize> {
    let mut units = 0usize;
    for (index, ch) in row.char_indices() {
        if units == character {
            return Ok(index);
        }
        if units > character {
            return Err(Error::Tool(
                "lsp edit column splits a UTF-16 surrogate pair".to_string(),
            ));
        }
        units += ch.len_utf16();
    }
    if units == character {
        return Ok(row.len());
    }
    Err(Error::Tool(
        "lsp edit column is past the end of the line".to_string(),
    ))
}

pub(super) fn format_location(item: &Value) -> Option<String> {
    let uri = item
        .get("uri")
        .or_else(|| item.get("targetUri"))
        .and_then(Value::as_str)?;
    let line = item
        .pointer("/range/start/line")
        .or_else(|| item.pointer("/targetRange/start/line"))
        .or_else(|| item.pointer("/targetSelectionRange/start/line"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let col = item
        .pointer("/range/start/character")
        .or_else(|| item.pointer("/targetRange/start/character"))
        .or_else(|| item.pointer("/targetSelectionRange/start/character"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let path = Url::parse(uri)
        .ok()
        .and_then(|url| url.to_file_path().ok())
        .map_or_else(|| uri.to_string(), |path| path.display().to_string());
    Some(format!("{path}:{line}:{col}"))
}

#[cfg(test)]
#[path = "proto_tests.rs"]
mod tests;
