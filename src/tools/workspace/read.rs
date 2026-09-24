use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::hashline::tagged_lines;

use super::image::image_type;
use super::{ToolRegistry, object_string, object_usize, parse_args};
use crate::tools::ToolOutput;

pub(super) const READ_DEFAULT_LIMIT: usize = 400;
const READ_PAGE_BYTES: usize = 50 * 1024;
const READ_MAX_LIMIT: usize = 1000;
const READ_MAX_BYTES: usize = 8 * 1024;
const READ_MAX_WORK_BYTES: usize = 8 * 1024 * 1024;

impl ToolRegistry {
    pub(super) fn read_files(&self, args: &Value, paths: &[Value]) -> Result<String> {
        if paths.is_empty() {
            return Err(Error::Tool("paths is empty".to_string()));
        }
        let mut out = String::new();
        for path in paths {
            let Some(path) = path.as_str() else {
                continue;
            };
            let mut call = serde_json::json!({"path": path});
            for key in ["offset", "limit"] {
                if let Some(value) = args.get(key) {
                    call[key] = value.clone();
                }
            }
            let call = call.to_string();
            match self.read_file(&call) {
                Ok(text) if !out.is_empty() && out.len() + text.len() > READ_PAGE_BYTES => {
                    out.push_str(&format!(
                        "{path}: not shown, the read passed {READ_PAGE_BYTES} bytes\n"
                    ));
                }
                Ok(text) => {
                    out.push_str(&self.dedup_read(&call, ToolOutput::ok(text)).text);
                    out.push('\n');
                }
                Err(error) => {
                    out.push_str(&format!("{path}: {error}\n"));
                }
            }
        }
        Ok(out.trim_end().to_string())
    }

    pub(super) fn read_file(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("read", arguments)?;
        let path = object_string(&args, "path")?;
        let offset = object_usize(&args, "offset").unwrap_or(1).max(1);
        let limit = object_usize(&args, "limit")
            .unwrap_or(READ_DEFAULT_LIMIT)
            .clamp(1, READ_MAX_LIMIT);
        let tags = args.get("tags").and_then(Value::as_bool).unwrap_or(false);
        let canonical = self.resolve_read(path)?;
        let file = std::fs::File::open(&canonical)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        let mut reader = BufReader::new(file);
        {
            let head = reader
                .fill_buf()
                .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
            if head.starts_with(b"%PDF-") {
                return Err(Error::Tool(format!(
                    "`{path}` is a PDF; run `pdftotext '{}' -` through run_command",
                    canonical.display()
                )));
            }
            if image_type(head).is_some() {
                let bytes = std::fs::read(&canonical)
                    .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
                let media = image_type(&bytes).unwrap_or("image");
                let (width, height) = ::image::ImageReader::new(std::io::Cursor::new(&bytes))
                    .with_guessed_format()
                    .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?
                    .into_dimensions()
                    .map_err(|error| Error::Tool(format!("cannot decode `{path}`: {error}")))?;
                return self.image_outcome(path, media, &bytes, width, height);
            }
        }
        let first = offset.saturating_sub(1);
        let mut window: Vec<String> = Vec::new();
        let mut window_bytes = 0_usize;
        let mut output_truncated = false;
        let mut line_clipped = false;
        let mut total_unknown = false;
        let mut total = 0_usize;
        let mut read_bytes = 0_usize;
        let mut raw = Vec::new();
        loop {
            raw.clear();
            let room = READ_MAX_WORK_BYTES
                .saturating_sub(read_bytes)
                .saturating_add(1);
            let count = reader
                .by_ref()
                .take(u64::try_from(room).unwrap_or(u64::MAX))
                .read_until(b'\n', &mut raw)
                .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
            if count == 0 {
                break;
            }
            let ended = raw.last() == Some(&b'\n');
            if total < first && !ended && count == room {
                return Err(Error::Tool(format!(
                    "`{path}` line {} is longer than {READ_MAX_WORK_BYTES} bytes; read it with offset {}",
                    total + 1,
                    total + 1
                )));
            }
            if total >= first {
                read_bytes = read_bytes.saturating_add(count);
            }
            let body = if ended {
                &raw[..raw.len() - 1]
            } else {
                &raw[..]
            };
            let over = read_bytes > READ_MAX_WORK_BYTES;
            if !output_truncated && total >= first && total < first.saturating_add(limit) {
                let (line, clipped) = decode(body, over && !ended);
                let size = line.len().saturating_add(1);
                if window.is_empty() || window_bytes.saturating_add(size) <= READ_PAGE_BYTES {
                    window_bytes = window_bytes.saturating_add(size);
                    line_clipped |= clipped;
                    window.push(line);
                } else {
                    output_truncated = true;
                }
            }
            total += 1;
            if over {
                total_unknown = true;
                break;
            }
        }
        if total == 0 && !total_unknown {
            return Ok(format!("{path}  lines 0-0/0"));
        }
        if first >= total && !total_unknown {
            return Err(Error::Tool(format!(
                "`{path}` has {total} lines; offset {offset} is past the end"
            )));
        }
        let shown = window.len();
        let body = tagged_lines(&window, first, total, tags);
        let body = if total_unknown {
            body.replacen(&format!("/{total}"), "/unknown", 1)
        } else {
            body
        };
        if total_unknown || output_truncated || (line_clipped && first + shown < total) {
            let notice = if total_unknown {
                format!(
                    "[truncated at {READ_MAX_WORK_BYTES} bytes; total line count unknown; continue with offset {}]",
                    first + shown + 1
                )
            } else {
                format!(
                    "[truncated at {READ_PAGE_BYTES} bytes after line {}; continue with offset {}]",
                    first + shown,
                    first + shown + 1
                )
            };
            return Ok(format!("{path}  {body}\n{notice}"));
        }
        Ok(format!("{path}  {body}"))
    }
}

fn decode(raw: &[u8], partial: bool) -> (String, bool) {
    let mut raw = raw;
    if raw.last() == Some(&b'\r') {
        raw = &raw[..raw.len() - 1];
    }
    let clipped = partial || raw.len() > READ_MAX_BYTES;
    let prefix = &raw[..raw.len().min(READ_MAX_BYTES)];
    let prefix = match std::str::from_utf8(prefix) {
        Err(error) if clipped && error.error_len().is_none() => &prefix[..error.valid_up_to()],
        Ok(_) | Err(_) => prefix,
    };
    let mut text = match String::from_utf8_lossy(prefix) {
        Cow::Borrowed(text) => text.to_owned(),
        Cow::Owned(text) => format!("{text} [invalid UTF-8 replaced]"),
    };
    if clipped {
        text.push_str(&format!("… [line clipped at {READ_MAX_BYTES} bytes]"));
    }
    (text, clipped)
}
