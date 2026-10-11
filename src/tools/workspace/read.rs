use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::hashline::{Tags, line_hash, tagged_lines};

use super::image::image_type;
use super::{ToolRegistry, object_string, object_usize, parse_args};
use crate::tools::ToolOutput;

const READ_PAGE_BYTES: usize = 50 * 1024;
const READ_MAX_BYTES: usize = 8 * 1024;
const READ_MAX_WORK_BYTES: usize = 8 * 1024 * 1024;

enum Opened {
    Text(BufReader<std::fs::File>),
    Image(ToolOutput),
}

#[derive(Default)]
struct Page {
    window: Vec<String>,
    hashes: Vec<String>,
    bytes: usize,
    total: usize,
    truncated: bool,
    clipped: bool,
    unknown: bool,
}

impl Page {
    fn wants(&self, first: usize, limit: usize) -> bool {
        !self.truncated && self.total >= first && self.total < first.saturating_add(limit)
    }

    fn keep(&mut self, line: String, hash: String, clipped: bool) {
        let size = line.len().saturating_add(1);
        if self.window.is_empty() || self.bytes.saturating_add(size) <= READ_PAGE_BYTES {
            self.bytes = self.bytes.saturating_add(size);
            self.clipped |= clipped;
            self.window.push(line);
            self.hashes.push(hash);
        } else {
            self.truncated = true;
        }
    }

    fn render(&self, path: &str, first: usize, tags: bool) -> Result<String> {
        if self.total == 0 && !self.unknown {
            return Ok(format!("{path} · empty file"));
        }
        if first >= self.total && !self.unknown {
            return Err(Error::Tool(format!(
                "`{path}` has {} lines; offset {} is past the end",
                self.total,
                first + 1
            )));
        }
        let shown = self.window.len();
        let tags = if tags {
            Tags::Given(&self.hashes)
        } else {
            Tags::Off
        };
        let body = tagged_lines(&self.window, first, self.total, tags);
        let body = if self.unknown {
            body.replacen(&format!("/{}", self.total), "/unknown", 1)
        } else {
            body
        };
        if self.unknown || self.truncated || (self.clipped && first + shown < self.total) {
            return Ok(format!("{path}  {body}\n{}", self.notice(first, shown)));
        }
        Ok(format!("{path}  {body}"))
    }

    fn notice(&self, first: usize, shown: usize) -> String {
        if self.unknown {
            return format!(
                "[truncated at {READ_MAX_WORK_BYTES} bytes; total line count unknown; continue with offset {}]",
                first + shown + 1
            );
        }
        format!(
            "[truncated at {READ_PAGE_BYTES} bytes after line {}; continue with offset {}]",
            first + shown,
            first + shown + 1
        )
    }
}

impl ToolRegistry {
    pub(super) fn read_files(&self, args: &Value, paths: &[Value]) -> Result<ToolOutput> {
        if paths.is_empty() {
            return Err(Error::Tool("paths is empty".to_string()));
        }
        let mut out = String::new();
        let mut images = Vec::new();
        let mut attempted = 0_usize;
        let mut failed = 0_usize;
        let mut first_failure: Option<String> = None;
        for path in paths {
            let Some(path) = path.as_str() else {
                continue;
            };
            attempted += 1;
            let mut call = args.clone();
            if let Some(fields) = call.as_object_mut() {
                fields.remove("paths");
            }
            call["path"] = Value::String(path.to_string());
            let call = call.to_string();
            match self.read_file(&call) {
                Ok(read) if !out.is_empty() && out.len() + read.text.len() > READ_PAGE_BYTES => {
                    out.push_str(&format!(
                        "{path}: not shown, the read passed {READ_PAGE_BYTES} bytes\n"
                    ));
                }
                Ok(read) => {
                    let read = self.dedup_read(&call, read);
                    out.push_str(&read.text);
                    out.push('\n');
                    images.extend(read.images);
                }
                Err(error) => {
                    failed += 1;
                    let text = error.to_string();
                    out.push_str(&format!("{path}: {text}\n"));
                    first_failure.get_or_insert(text);
                }
            }
        }
        if attempted > 0 && attempted == failed {
            return Err(Error::Tool(first_failure.unwrap_or_default()));
        }
        Ok(ToolOutput {
            images,
            ..ToolOutput::ok(out.trim_end().to_string())
        })
    }

    pub(super) fn read_file(&self, arguments: &str) -> Result<ToolOutput> {
        let args: Value = parse_args("read", arguments)?;
        let path = object_string(&args, "path")?;
        let context = object_usize(&args, "context");
        let before = object_usize(&args, "before").or(context).unwrap_or(0);
        let after = object_usize(&args, "after").or(context).unwrap_or(0);
        let offset = object_usize(&args, "offset").unwrap_or(1).max(1);
        let limit = object_usize(&args, "limit").map_or(usize::MAX, |limit| {
            limit
                .saturating_add(before.min(offset - 1))
                .saturating_add(after)
                .clamp(1, crate::limits::limits().read_line_limit)
        });
        let tags = args.get("tags").and_then(Value::as_bool).unwrap_or(false);
        let mut reader = match self.open_text(path)? {
            Opened::Text(reader) => reader,
            Opened::Image(image) => return Ok(image),
        };
        let first = offset.saturating_sub(1).saturating_sub(before);
        scan(&mut reader, path, first, limit, tags)?
            .render(path, first, tags)
            .map(ToolOutput::ok)
    }

    fn open_text(&self, path: &str) -> Result<Opened> {
        let canonical = self.resolve_read(path, crate::tools::paths::Shield::Contained)?;
        let file = std::fs::File::open(&canonical)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        let mut reader = BufReader::new(file);
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
            return self.image_page(path, &canonical).map(Opened::Image);
        }
        Ok(Opened::Text(reader))
    }

    fn image_page(&self, path: &str, canonical: &Path) -> Result<ToolOutput> {
        let bytes = std::fs::read(canonical)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        let media = image_type(&bytes).unwrap_or("image");
        let (width, height) = ::image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?
            .into_dimensions()
            .map_err(|error| Error::Tool(format!("cannot decode `{path}`: {error}")))?;
        self.image_outcome(path, media, &bytes, width, height)
    }
}

fn scan(
    reader: &mut BufReader<std::fs::File>,
    path: &str,
    first: usize,
    limit: usize,
    tags: bool,
) -> Result<Page> {
    let mut page = Page::default();
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
        if page.total < first && !ended && count == room {
            return Err(Error::Tool(format!(
                "`{path}` line {} is longer than {READ_MAX_WORK_BYTES} bytes; read it with offset {}",
                page.total + 1,
                page.total + 1
            )));
        }
        if page.total >= first {
            read_bytes = read_bytes.saturating_add(count);
        }
        let body = if ended {
            &raw[..raw.len() - 1]
        } else {
            &raw[..]
        };
        let over = read_bytes > READ_MAX_WORK_BYTES;
        if page.wants(first, limit) {
            let hash = if tags {
                line_hash(body.strip_suffix(b"\r").unwrap_or(body))
            } else {
                String::new()
            };
            let (line, clipped) = decode(body, over && !ended);
            page.keep(line, hash, clipped);
        }
        page.total += 1;
        if over {
            page.unknown = true;
            break;
        }
    }
    Ok(page)
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

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
