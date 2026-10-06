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
            match item.get("kind").and_then(Value::as_str) {
                Some("create") => {
                    let uri = item
                        .get("uri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Tool("lsp create is missing a uri".to_string()))?;
                    ops.push(Change::Create {
                        uri: uri.to_string(),
                    });
                }
                Some("rename") => {
                    let from = item
                        .get("oldUri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Tool("lsp rename is missing oldUri".to_string()))?;
                    let to = item
                        .get("newUri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Tool("lsp rename is missing newUri".to_string()))?;
                    ops.push(Change::Rename {
                        from: from.to_string(),
                        to: to.to_string(),
                    });
                }
                Some("delete") => {
                    let uri = item
                        .get("uri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Tool("lsp delete is missing a uri".to_string()))?;
                    ops.push(Change::Delete {
                        uri: uri.to_string(),
                    });
                }
                Some(other) => {
                    return Err(Error::Tool(format!(
                        "lsp rename includes unsupported file operation `{other}`"
                    )));
                }
                None => {
                    let uri = item
                        .pointer("/textDocument/uri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            Error::Tool("lsp rename is missing a document uri".to_string())
                        })?;
                    let edits = item
                        .get("edits")
                        .map(parse_edits)
                        .transpose()?
                        .unwrap_or_default();
                    if !edits.is_empty() {
                        ops.push(Change::Text {
                            uri: uri.to_string(),
                            edits,
                        });
                    }
                }
            }
        }
    }
    Ok(ops)
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
    let mut out = text.to_string();
    for edit in ordered.iter().rev() {
        let start = offset(&out, edit.start_line, edit.start_col)?;
        let end = offset(&out, edit.end_line, edit.end_col)?;
        if start > end {
            return Err(Error::Tool("lsp edit range is inverted".to_string()));
        }
        out.replace_range(start..end, &edit.new_text);
    }
    Ok(out)
}

pub(super) fn offset(text: &str, line: usize, character: usize) -> Result<usize> {
    let mut pos = 0usize;
    let mut rest = text;
    for _ in 0..line {
        let Some(index) = rest.find('\n') else {
            return Err(Error::Tool(
                "lsp edit line is past the end of the file".to_string(),
            ));
        };
        pos += index + 1;
        rest = &rest[index + 1..];
    }
    let row = rest.split('\n').next().unwrap_or(rest);
    let mut units = 0usize;
    for (index, ch) in row.char_indices() {
        if units == character {
            return Ok(pos + index);
        }
        if units > character {
            return Err(Error::Tool(
                "lsp edit column splits a UTF-16 surrogate pair".to_string(),
            ));
        }
        units += ch.len_utf16();
    }
    if units == character {
        return Ok(pos + row.len());
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
    let path = uri
        .strip_prefix("file://")
        .unwrap_or(uri)
        .replace("%20", " ");
    Some(format!("{path}:{line}:{col}"))
}

pub(super) async fn write_msg(writer: &mut (impl AsyncWrite + Unpin), value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|error| Error::Tool(format!("lsp write: {error}")))?;
    writer
        .write_all(&body)
        .await
        .map_err(|error| Error::Tool(format!("lsp write: {error}")))?;
    writer
        .flush()
        .await
        .map_err(|error| Error::Tool(format!("lsp write: {error}")))?;
    Ok(())
}

pub(super) async fn read_msg(reader: &mut BufReader<impl AsyncRead + Unpin>) -> Result<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|error| Error::Tool(format!("lsp read: {error}")))?;
        if n == 0 {
            return Err(Error::Tool("language server closed".to_string()));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .map(str::trim)
        {
            length = value.parse().ok();
        }
    }
    let length =
        length.ok_or_else(|| Error::Tool("lsp message missing Content-Length".to_string()))?;
    let mut body = vec![0_u8; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|error| Error::Tool(format!("lsp read: {error}")))?;
    serde_json::from_slice(&body).map_err(|error| Error::Tool(format!("lsp json: {error}")))
}

#[cfg(test)]
#[path = "proto_tests.rs"]
mod tests;
