use std::io::{BufReader, Read, Write};
use std::path::Path;

use regex::Regex;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::hashline::{
    dirty_span, line_hash, resolve_span, tagged_lines, tagged_span, tagged_text,
};
use crate::provider::{Image, Source, UserInput};

use super::{ToolOutput, ToolRegistry, nonempty_path, object_string, object_usize, parse_args};
use crate::SKIP_DIRS;

const READ_DEFAULT_LIMIT: usize = 300;
const READ_MAX_LIMIT: usize = 1000;
const READ_MAX_BYTES: usize = 2 * 1024 * 1024;
const READ_MAX_WORK_BYTES: usize = 8 * 1024 * 1024;
const SMALL_FILE_LINES: usize = 80;
const LIST_MAX: usize = 200;
const GREP_DEFAULT: usize = 50;
const GREP_MAX: usize = 200;
const GREP_FILE_MAX: usize = 1024 * 1024;
pub(crate) const IMAGES_MAX_BYTES: usize = 10 * 1024 * 1024;

impl ToolRegistry {
    pub(super) fn read_file(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("read_file", arguments)?;
        let path = object_string(&args, "path")?;
        let offset = object_usize(&args, "offset").unwrap_or(1).max(1);
        let limit = object_usize(&args, "limit")
            .unwrap_or(READ_DEFAULT_LIMIT)
            .clamp(1, READ_MAX_LIMIT);
        let canonical = self.resolve_read(path)?;
        let file = std::fs::File::open(&canonical)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        let first = offset.saturating_sub(1);
        let mut window: Vec<String> = Vec::new();
        let mut window_bytes = 0_usize;
        let mut output_truncated = false;
        let mut line_clipped = false;
        let file_bytes = file.metadata()?.len();
        let mut bytes = Vec::with_capacity(file_bytes.min(READ_MAX_WORK_BYTES as u64) as usize);
        BufReader::new(file)
            .take(READ_MAX_WORK_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        if let Some(media) = image_type(&bytes) {
            let (width, height) = ::image::ImageReader::new(std::io::Cursor::new(&bytes))
                .with_guessed_format()
                .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?
                .into_dimensions()
                .map_err(|error| Error::Tool(format!("cannot decode `{path}`: {error}")))?;
            return Ok(format!(
                "{path} is a {media} image, {width}x{height}. It is shown to the user. To look at it yourself, ask the user to attach it with @{path}."
            ));
        }
        let total_unknown = bytes.len() > READ_MAX_WORK_BYTES;
        bytes.truncate(READ_MAX_WORK_BYTES);
        let ends_newline = bytes.last() == Some(&b'\n');
        let mut total = 0_usize;
        let mut lines = bytes.split(|byte| *byte == b'\n').peekable();
        while let Some(raw) = lines.next() {
            let final_segment = lines.peek().is_none();
            if raw.is_empty() && final_segment && (ends_newline || bytes.is_empty()) {
                break;
            }
            let partial = total_unknown && !ends_newline && final_segment;
            if !output_truncated && total >= first && total < first.saturating_add(limit) {
                let (line, clipped) = decode(path, total, raw, partial)?;
                let size = line.len().saturating_add(1);
                if window.is_empty() || window_bytes.saturating_add(size) <= READ_MAX_BYTES {
                    window_bytes = window_bytes.saturating_add(size);
                    line_clipped |= clipped;
                    window.push(line);
                } else {
                    output_truncated = true;
                }
            }
            total += 1;
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
        if shown == 0 && total_unknown {
            return Err(Error::Tool(format!(
                "cannot reach offset {offset} within {READ_MAX_WORK_BYTES} bytes; total line count is unknown"
            )));
        }
        let body = tagged_lines(&window, first, total);
        let body = if total_unknown {
            body.replacen(&format!("/{total}"), "/unknown", 1)
        } else {
            body
        };
        if total_unknown || output_truncated || (line_clipped && first + shown < total) {
            let notice = if total_unknown {
                format!(
                    "[truncated at {} bytes; total line count unknown; continue with offset {}]",
                    bytes.len(),
                    first + shown + 1
                )
            } else {
                format!(
                    "[truncated at {READ_MAX_BYTES} bytes after line {}; continue with offset {}]",
                    first + shown,
                    first + shown + 1
                )
            };
            return Ok(format!("{path}  {body}\n{notice}"));
        }
        Ok(format!("{path}  {body}"))
    }

    pub(super) async fn edit_file(&self, arguments: &str) -> Result<ToolOutput> {
        let args: Value = parse_args("edit_file", arguments)?;
        self.apply_edit("edit_file", &args).await
    }

    pub(super) async fn edit_operation(&self, name: &str, arguments: &str) -> Result<ToolOutput> {
        let mut args: Value = parse_args(name, arguments)?;
        if name == "delete_lines" {
            args.as_object_mut()
                .ok_or_else(|| Error::Tool(format!("{name} arguments must be an object")))?
                .insert("op".to_string(), Value::String("delete".to_string()));
        }
        self.apply_edit(name, &args).await
    }

    async fn apply_edit(&self, name: &str, args: &Value) -> Result<ToolOutput> {
        let path = nonempty_path(args, name)?;
        let dest = self.resolve_existing(path)?;
        self.policy.write_path(&self.relative(&dest)?)?;
        let raw = std::fs::read_to_string(&dest)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        if raw.is_empty() {
            return Err(Error::Tool(format!(
                "`{path}` is empty — there is no line to anchor an edit; use write_file"
            )));
        }
        let endings = terminators(&raw);
        let original = raw.replace("\r\n", "\n");
        let edited = match args.get("edits").and_then(Value::as_array) {
            Some(ops) if !ops.is_empty() && name == "replace_texts" => {
                apply_sequence(&original, ops)?
            }
            Some(ops) if !ops.is_empty() => apply_many(&original, ops)?,
            Some(_) => return Err(Error::Tool("edits is empty".to_string())),
            None => apply_one(&original, args)
                .map_err(|error| edit_error_context(error, &original, args))?,
        };
        let edited = restore(&endings, &original, &edited);
        atomic_write(&dest, path, &edited)?;
        let mut out = format!("edited {path}\n{}", edit_view(&original, &edited));
        if let Some(check) = self.check_after_edit(&dest).await {
            out.push('\n');
            out.push_str(&check.text);
        }
        Ok(ToolOutput {
            text: out,
            success: true,
            mutated: true,
        })
    }

    pub(super) async fn write_file(&self, arguments: &str) -> Result<ToolOutput> {
        let args: Value = parse_args("write_file", arguments)?;
        let path = nonempty_path(&args, "write_file")?;
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Tool("write_file requires content".to_string()))?;
        let dest = self.resolve_write(path)?;
        self.policy.write_globs(&self.relative(&dest)?)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::Tool(format!("cannot create `{path}`: {error}")))?;
        }
        atomic_write(&dest, path, content)?;
        let lines = content.lines().count();
        let view = if lines <= SMALL_FILE_LINES {
            tagged_text(content)
        } else {
            let tail = lines.saturating_sub(11);
            format!(
                "{}\n…\n{}",
                tagged_span(content, 0, 19),
                tagged_span(content, tail, usize::MAX)
            )
        };
        let mut out = format!("wrote {path} ({lines} lines)\n{view}");
        if let Some(check) = self.check_after_edit(&dest).await {
            out.push('\n');
            out.push_str(&check.text);
        }
        Ok(ToolOutput {
            text: out,
            success: true,
            mutated: true,
        })
    }

    pub(super) fn list_dir(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("list_dir", arguments)?;
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .unwrap_or(".");
        let dir = if path == "." {
            self.workspace.clone()
        } else {
            self.resolve_read(path)?
        };
        if !dir.is_dir() {
            return Err(Error::Tool(format!("`{path}` is not a directory")));
        }
        format_directory(path, &dir)
    }

    pub(super) fn grep(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("grep", arguments)?;
        let pattern = args.get("pattern").and_then(Value::as_str).unwrap_or("");
        let glob = args
            .get("glob")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|glob| !glob.is_empty());
        if pattern.is_empty() && glob.is_none() {
            return Err(Error::Tool("grep requires pattern or glob".to_string()));
        }
        let limit = object_usize(&args, "limit")
            .unwrap_or(GREP_DEFAULT)
            .clamp(1, GREP_MAX);
        let root = match args.get("path").and_then(Value::as_str) {
            Some(path) if !path.trim().is_empty() && path != "." => self.resolve_read(path)?,
            _ => self.workspace.clone(),
        };
        let regex = if pattern.is_empty() {
            None
        } else {
            Some(match Regex::new(pattern) {
                Ok(regex) => regex,
                Err(_) => Regex::new(&regex::escape(pattern))
                    .map_err(|error| Error::Tool(format!("grep pattern is invalid: {error}")))?,
            })
        };
        let glob = glob.map(glob_regex).transpose()?;
        let mut hits = Vec::new();
        let mut scanned = 0_usize;
        if root.is_file() {
            grep_file(
                &root,
                &self.workspace,
                regex.as_ref(),
                glob.as_ref(),
                limit,
                &mut hits,
                &mut scanned,
            )?;
        } else {
            let base = self
                .workspace
                .canonicalize()
                .unwrap_or(self.workspace.clone());
            let root = root.canonicalize().unwrap_or(root);
            let base = if root.starts_with(&base) {
                base
            } else {
                root.clone()
            };
            let mut files = if root == base {
                crate::durable::tree::listing(&base)
            } else {
                crate::durable::tree::walked(&root).map(|found| {
                    found
                        .into_iter()
                        .filter_map(|rel| {
                            root.join(rel)
                                .strip_prefix(&base)
                                .ok()
                                .map(Path::to_path_buf)
                        })
                        .collect()
                })
            }
            .map_err(|error| Error::Tool(format!("cannot search the workspace: {error}")))?;
            files.sort();
            for rel in files {
                if hits.len() >= limit {
                    break;
                }
                let path = base.join(&rel);
                if !path.starts_with(&root)
                    || rel.components().any(|part| {
                        SKIP_DIRS.contains(&part.as_os_str().to_string_lossy().as_ref())
                    })
                    || !path
                        .symlink_metadata()
                        .is_ok_and(|metadata| metadata.is_file())
                {
                    continue;
                }
                grep_file(
                    &path,
                    &base,
                    regex.as_ref(),
                    glob.as_ref(),
                    limit,
                    &mut hits,
                    &mut scanned,
                )?;
            }
        }
        if hits.is_empty() {
            if pattern.is_empty() {
                return Ok(format!("no files match ({scanned} files)"));
            }
            return Ok(format!("no matches for `{pattern}` ({scanned} files)"));
        }
        let shown = hits.len().min(limit);
        let mut out = if pattern.is_empty() {
            format!("{} files ({scanned} scanned)", hits.len())
        } else {
            format!("{} matches ({scanned} files)", hits.len())
        };
        if hits.len() > limit {
            out.push_str(&format!(", showing {limit}"));
        }
        for hit in hits.into_iter().take(shown) {
            out.push('\n');
            out.push_str(&hit);
        }
        Ok(out)
    }

    pub(super) fn skill(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("skill", arguments)?;
        let name = object_string(&args, "name")?;
        crate::skills::load(&self.workspace, name)
    }
}

fn format_directory(path: &str, dir: &Path) -> Result<String> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)
        .map_err(|error| Error::Tool(format!("cannot list `{path}`: {error}")))?
    {
        let entry = entry.map_err(|error| Error::Tool(format!("cannot list `{path}`: {error}")))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let meta = entry.metadata().ok();
        let kind = if meta.as_ref().is_some_and(std::fs::Metadata::is_dir) {
            "dir"
        } else {
            "file"
        };
        let size = meta
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len().to_string())
            .unwrap_or_default();
        entries.push((name, kind, size));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let total = entries.len();
    entries.truncate(LIST_MAX);
    let mut out = format!("{path}  {total} entries");
    for (name, kind, size) in entries {
        out.push('\n');
        out.push_str(&name);
        if kind == "dir" {
            out.push('/');
        }
        out.push('\t');
        out.push_str(kind);
        if !size.is_empty() {
            out.push('\t');
            out.push_str(&size);
        }
    }
    if total > LIST_MAX {
        out.push_str(&format!("\n… {omit} more", omit = total - LIST_MAX));
    }
    Ok(out)
}

pub(super) fn atomic_write(dest: &Path, display: &str, content: &str) -> Result<()> {
    let parent = dest
        .parent()
        .ok_or_else(|| Error::Tool(format!("cannot write `{display}` without a parent")))?;
    let existing_mode = match std::fs::metadata(dest) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(Error::Tool(format!(
                "cannot inspect `{display}` permissions: {error}"
            )));
        }
    };
    let mut temp = tempfile::Builder::new()
        .prefix(".mage-tmp-")
        .tempfile_in(parent)
        .map_err(|error| Error::Tool(format!("cannot prepare `{display}`: {error}")))?;
    temp.write_all(content.as_bytes())
        .map_err(|error| Error::Tool(format!("cannot write `{display}`: {error}")))?;
    if let Some(permissions) = existing_mode {
        temp.as_file()
            .set_permissions(permissions)
            .map_err(|error| {
                Error::Tool(format!("cannot preserve `{display}` permissions: {error}"))
            })?;
    }
    temp.persist(dest)
        .map(|_| ())
        .map_err(|error| Error::Tool(format!("cannot replace `{display}`: {error}")))
}

fn decode(path: &str, index: usize, raw: &[u8], partial: bool) -> Result<(String, bool)> {
    let mut raw = raw;
    if raw.last() == Some(&b'\r') {
        raw = &raw[..raw.len() - 1];
    }
    let clipped = partial || raw.len() > READ_MAX_BYTES;
    let prefix = &raw[..raw.len().min(READ_MAX_BYTES)];
    let decoded = match std::str::from_utf8(prefix) {
        Err(error) if clipped && error.error_len().is_none() => {
            std::str::from_utf8(&prefix[..error.valid_up_to()])
        }
        result => result,
    };
    let mut text = decoded
        .map_err(|error| {
            Error::Tool(format!(
                "`{path}` is not valid UTF-8 at line {}: {error}",
                index + 1
            ))
        })?
        .to_owned();
    if clipped {
        text.push_str(&format!("… [line clipped at {READ_MAX_BYTES} bytes]"));
    }
    Ok((text, clipped))
}

impl ToolRegistry {
    pub(super) fn show_image(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("show_image", arguments)?;
        let path = object_string(&args, "path")?;
        let canonical = self.resolve_read(path)?;
        let bytes = std::fs::read(&canonical)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        let media = image_type(&bytes).ok_or_else(|| {
            Error::Tool(format!("`{path}` is not a PNG, JPEG, GIF, or WebP image"))
        })?;
        let (width, height) = ::image::load_from_memory(&bytes)
            .map(|decoded| (decoded.width(), decoded.height()))
            .map_err(|error| Error::Tool(format!("cannot decode `{path}`: {error}")))?;
        Ok(format!("shown {path} ({media}, {width}x{height})"))
    }
}

pub(super) fn attach(workspace: &Path, line: &str) -> Result<UserInput> {
    let mut out = String::new();
    let mut images = Vec::new();
    let mut image_bytes = 0_usize;
    let mut attached = false;
    for word in words(line) {
        let path = word.strip_prefix('@').map(|path| {
            if path.starts_with('"') {
                serde_json::from_str::<String>(path).unwrap_or_else(|_| path.to_string())
            } else {
                path.to_string()
            }
        });
        if let Some(path) = path.as_deref()
            && !path.is_empty()
        {
            let full = workspace.join(path);
            if !full.is_file() {
                if !word.starts_with("@\"") {
                    continue;
                }
                return Err(Error::Tool(format!(
                    "attachment `{path}` is not a readable file; check the path or remove the @ reference"
                )));
            }
            let root = workspace
                .canonicalize()
                .map_err(|error| Error::Tool(format!("cannot open workspace: {error}")))?;
            let canonical = full.canonicalize().map_err(|error| {
                Error::Tool(format!("cannot open attachment `{path}`: {error}"))
            })?;
            if !canonical.starts_with(&root) {
                return Err(Error::Tool(format!(
                    "attachment `{path}` is outside the workspace"
                )));
            }
            let bytes = std::fs::read(&canonical).map_err(|error| {
                Error::Tool(format!("cannot read attachment `{path}`: {error}"))
            })?;
            if let Some(media_type) = image_type(&bytes) {
                image_bytes = image_bytes.saturating_add(bytes.len());
                if image_bytes > IMAGES_MAX_BYTES {
                    return Err(Error::Tool(format!(
                        "attached images are larger than {IMAGES_MAX_BYTES} bytes"
                    )));
                }
                images.push(Image::encode(media_type, &bytes));
            } else {
                let text = String::from_utf8(bytes).map_err(|error| {
                    Error::Tool(format!(
                        "`{path}` is not a supported image or UTF-8 text: {error}"
                    ))
                })?;
                out.push_str(&format!("\n--- {path} ---\n{text}\n"));
            }
            attached = true;
        }
    }
    let text = if out.is_empty() {
        line.to_string()
    } else {
        format!("{line}\n{out}")
    };
    let source = attached.then(|| Source {
        text: line.to_string(),
        images: images.len(),
    });
    Ok(UserInput {
        text,
        images,
        source,
    })
}

fn words(mut line: &str) -> Vec<&str> {
    let mut words = Vec::new();
    loop {
        line = line.trim_start();
        if line.is_empty() {
            return words;
        }
        let quoted = line.strip_prefix("@\"").and_then(|_| {
            let mut parsed = serde_json::Deserializer::from_str(&line[1..]).into_iter::<String>();
            matches!(parsed.next(), Some(Ok(_))).then(|| parsed.byte_offset() + 1)
        });
        let end = quoted.unwrap_or_else(|| line.find(char::is_whitespace).unwrap_or(line.len()));
        words.push(&line[..end]);
        line = &line[end..];
    }
}

pub(crate) fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

fn edit_view(before: &str, after: &str) -> String {
    let lines = after.lines().count();
    if lines <= SMALL_FILE_LINES {
        return tagged_text(after);
    }
    let (first, last) = dirty_span(before, after);
    let from = first.saturating_sub(2);
    let to = last.saturating_add(2);
    tagged_span(after, from, to)
}

fn grep_file(
    path: &Path,
    workspace: &Path,
    regex: Option<&Regex>,
    glob: Option<&Regex>,
    limit: usize,
    hits: &mut Vec<String>,
    scanned: &mut usize,
) -> Result<()> {
    let rel = path
        .strip_prefix(workspace)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    if let Some(glob) = glob
        && !glob.is_match(&rel)
    {
        return Ok(());
    }
    *scanned += 1;
    let Some(regex) = regex else {
        if hits.len() < limit {
            hits.push(rel);
        }
        return Ok(());
    };
    let meta = path
        .metadata()
        .map_err(|error| Error::Tool(format!("cannot search `{}`: {error}", path.display())))?;
    if meta.len() > GREP_FILE_MAX as u64 {
        return Ok(());
    }
    let bytes = std::fs::read(path)
        .map_err(|error| Error::Tool(format!("cannot read `{}`: {error}", path.display())))?;
    if bytes.contains(&0) {
        return Ok(());
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(());
    };
    for (index, line) in text.lines().enumerate() {
        if hits.len() >= limit {
            break;
        }
        if regex.is_match(line) {
            hits.push(format!("{rel}:{}:{}|{line}", index + 1, line_hash(line)));
        }
    }
    Ok(())
}

fn glob_regex(glob: &str) -> Result<Regex> {
    let glob = glob.replace('\\', "/");
    let converted = glob_to_regex(&glob);
    let pattern = if glob.contains('/') || glob.starts_with("**") {
        format!("^{converted}$")
    } else {
        format!("^(?:.*/)?{converted}$")
    };
    Regex::new(&pattern).map_err(|error| Error::Tool(format!("grep glob is invalid: {error}")))
}

fn glob_to_regex(glob: &str) -> String {
    let bytes = glob.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'*' if bytes.get(i + 1) == Some(&b'*') => {
                if bytes.get(i + 2) == Some(&b'/') {
                    out.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    out.push_str(".*");
                    i += 2;
                }
            }
            b'*' => {
                out.push_str("[^/]*");
                i += 1;
            }
            b'?' => {
                out.push_str("[^/]");
                i += 1;
            }
            c @ (b'.' | b'+' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'^' | b'$' | b'|'
            | b'\\') => {
                out.push('\\');
                out.push(c as char);
                i += 1;
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

fn edit_error_context(error: Error, text: &str, args: &Value) -> Error {
    let Error::Tool(message) = error else {
        return error;
    };
    let Some(tag) = args.get("tag").and_then(Value::as_str) else {
        return Error::Tool(message);
    };
    let Ok((line, _)) = resolve_span(text, tag, None) else {
        return Error::Tool(message);
    };
    Error::Tool(format!(
        "{message}\nCurrent lines:\n{}",
        tagged_span(text, line.saturating_sub(2), line.saturating_add(2))
    ))
}

fn apply_one(text: &str, op: &Value) -> Result<String> {
    apply_patches(text, vec![parse_patch(text, op)?])
}

struct Patch {
    start: usize,
    end: usize,
    anchor_start: usize,
    anchor_end: usize,
    replacement: Vec<String>,
}

fn parse_patch(text: &str, op: &Value) -> Result<Patch> {
    if let Some(tag) = op.get("replace_text").and_then(Value::as_str) {
        return replace_text_patch(
            text,
            Some(tag),
            object_string(op, "old")?,
            object_string(op, "new")?,
        );
    }
    if op.get("old").is_some() && op.get("new").is_some() {
        return replace_text_patch(
            text,
            optional_tag(op)?,
            object_string(op, "old")?,
            object_string(op, "new")?,
        );
    }
    if op.get("tag").is_some() {
        let tag = object_string(op, "tag")?;
        if op.get("new").is_some() {
            return replace_patch(
                text,
                tag,
                op.get("end_tag").and_then(Value::as_str),
                object_string(op, "new")?,
            );
        }
        if op.get("text").is_some() {
            return insert_patch(text, tag, object_string(op, "text")?);
        }
        if op.get("op").and_then(Value::as_str) == Some("delete") {
            return delete_patch(text, tag, op.get("end_tag").and_then(Value::as_str));
        }
    }
    if let Some(replace) = op.get("replace") {
        if !replace.is_object() {
            return Err(Error::Tool(
                "replace must be an object with tag, new, and optional end_tag; use replace_text with old and new for exact text"
                    .to_string(),
            ));
        }
        let tag = object_string(replace, "tag")?;
        let new = replace
            .get("new")
            .or_else(|| replace.get("text"))
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Tool("replace requires new".to_string()))?;
        let end = replace.get("end_tag").and_then(Value::as_str);
        return replace_patch(text, tag, end, new);
    }
    if let Some(replace) = op.get("replace_text") {
        if !replace.is_object() {
            return Err(Error::Tool(
                "replace_text must be an object with old, new, and optional tag".to_string(),
            ));
        }
        return replace_text_patch(
            text,
            optional_tag(replace)?,
            object_string(replace, "old")?,
            object_string(replace, "new")?,
        );
    }
    if let Some(insert) = op.get("insert_after") {
        if !insert.is_object() {
            return Err(Error::Tool(
                "insert_after must be an object with tag and text".to_string(),
            ));
        }
        let tag = object_string(insert, "tag")?;
        let body = object_string(insert, "text")?;
        return insert_patch(text, tag, body);
    }
    if let Some(delete) = op.get("delete") {
        if !delete.is_object() {
            return Err(Error::Tool(
                "delete must be an object with tag and optional end_tag".to_string(),
            ));
        }
        let tag = object_string(delete, "tag")?;
        let end = delete.get("end_tag").and_then(Value::as_str);
        return delete_patch(text, tag, end);
    }
    Err(Error::Tool(
        "edit_file requires replace_text, replace, insert_after, delete, or edits".to_string(),
    ))
}

fn optional_tag(value: &Value) -> Result<Option<&str>> {
    match value.get("tag") {
        None => Ok(None),
        Some(Value::String(tag)) if !tag.trim().is_empty() => Ok(Some(tag)),
        Some(_) => Err(Error::Tool("tag must be a non-empty string".to_string())),
    }
}

fn replace_text_patch(text: &str, tag: Option<&str>, old: &str, new: &str) -> Result<Patch> {
    if old.is_empty() {
        return Err(Error::Tool("replace_text.old is empty".to_string()));
    }
    let global_matches = count_matches(text, old);
    let (byte, line_index) = if global_matches == 1 {
        let byte = text.find(old).ok_or_else(|| {
            Error::Tool("replace_text.old disappeared while resolving the edit".to_string())
        })?;
        (
            byte,
            text[..byte].bytes().filter(|byte| *byte == b'\n').count(),
        )
    } else {
        match tag {
            Some(tag) => {
                let line_index = resolve_span(text, tag, None)
                    .or_else(|error| relocate_exact_anchor(text, tag).ok_or(error))?
                    .0;
                let line_start = line_byte(text, line_index);
                let line_end = text[line_start..]
                    .find('\n')
                    .map_or(text.len(), |offset| line_start + offset);
                let matches = text
                    .match_indices(old)
                    .filter(|(byte, _)| *byte >= line_start && *byte <= line_end)
                    .collect::<Vec<_>>();
                if matches.len() != 1 {
                    return Err(Error::Tool(format!(
                        "replace_text.old must start exactly once on the anchored line; found {}",
                        matches.len()
                    )));
                }
                (matches[0].0, line_index)
            }
            None => {
                return Err(Error::Tool(format!(
                    "replace_text.old must occur exactly once in the file when tag is omitted; found {global_matches}"
                )));
            }
        }
    };
    let end_byte = byte + old.len();
    let end_line = text.as_bytes()[..end_byte]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count();
    let start_byte = line_byte(text, line_index);
    let line_end = text[end_byte..]
        .find('\n')
        .map_or(text.len(), |offset| end_byte + offset);
    let suffix = &text[end_byte..line_end];
    let replacement = format!("{}{new}{suffix}", &text[start_byte..byte]);
    Ok(Patch {
        start: line_index,
        end: end_line + 1,
        anchor_start: line_index,
        anchor_end: end_line + 1,
        replacement: if replacement.is_empty() && !old.ends_with('\n') {
            vec![String::new()]
        } else {
            lines(&replacement)
        },
    })
}

fn relocate_exact_anchor(text: &str, tag: &str) -> Option<(usize, usize)> {
    let anchor = tag.split_once('|').map_or(tag, |(anchor, _)| anchor.trim());
    let (number, hash) = anchor.split_once(':')?;
    if number.parse::<usize>().ok()? == 0 {
        return None;
    }
    resolve_span(text, hash, None).ok()
}

fn line_byte(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    text.match_indices('\n')
        .nth(line - 1)
        .map_or(0, |(byte, _)| byte + 1)
}

fn count_matches(text: &str, needle: &str) -> usize {
    text.match_indices(needle).count()
}

fn replace_patch(text: &str, tag: &str, end_tag: Option<&str>, new: &str) -> Result<Patch> {
    let (start, end) = resolve_span(text, tag, end_tag)?;
    Ok(Patch {
        start,
        end: end + 1,
        anchor_start: start,
        anchor_end: end + 1,
        replacement: lines(new),
    })
}

fn insert_patch(text: &str, tag: &str, body: &str) -> Result<Patch> {
    if body.is_empty() {
        return Err(Error::Tool("insert_after.text is empty".to_string()));
    }
    let (anchor, _) = resolve_span(text, tag, None)?;
    Ok(Patch {
        start: anchor + 1,
        end: anchor + 1,
        anchor_start: anchor,
        anchor_end: anchor + 1,
        replacement: lines(body),
    })
}

fn delete_patch(text: &str, tag: &str, end_tag: Option<&str>) -> Result<Patch> {
    let (start, end) = resolve_span(text, tag, end_tag)?;
    Ok(Patch {
        start,
        end: end + 1,
        anchor_start: start,
        anchor_end: end + 1,
        replacement: Vec::new(),
    })
}

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

fn apply_many(original: &str, ops: &[Value]) -> Result<String> {
    let mut patches = Vec::with_capacity(ops.len());
    for (index, op) in ops.iter().enumerate() {
        let patch = parse_patch(original, op).map_err(|error| {
            match edit_error_context(error, original, op) {
                Error::Tool(message) => Error::Tool(format!("edit {}: {message}", index + 1)),
                other => other,
            }
        })?;
        patches.push(patch);
    }
    apply_patches(original, patches)
}

fn apply_sequence(original: &str, ops: &[Value]) -> Result<String> {
    let mut edited = original.to_string();
    for (index, op) in ops.iter().enumerate() {
        let patch = parse_patch(&edited, op).map_err(|error| {
            match edit_error_context(error, &edited, op) {
                Error::Tool(message) => Error::Tool(format!("edit {}: {message}", index + 1)),
                other => other,
            }
        })?;
        edited = apply_patches(&edited, vec![patch])?;
    }
    Ok(edited)
}

fn apply_patches(original: &str, mut patches: Vec<Patch>) -> Result<String> {
    patches.sort_by_key(|patch| patch.anchor_start);
    for pair in patches.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if right.anchor_start < left.anchor_end {
            return Err(Error::Tool(
                "edits overlap — merge them into one replace".to_string(),
            ));
        }
    }
    patches.sort_by_key(|patch| (patch.start, patch.start != patch.end));
    let mut lines = lines(original);
    for patch in patches.into_iter().rev() {
        lines.splice(patch.start..patch.end, patch.replacement);
    }
    let mut text = lines.join("\n");
    if original.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

fn terminators(raw: &str) -> Vec<&'static str> {
    let mut endings = Vec::new();
    let mut rest = raw;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(at) => {
                let crlf = at > 0 && rest.as_bytes()[at - 1] == b'\r';
                endings.push(if crlf { "\r\n" } else { "\n" });
                rest = &rest[at + 1..];
            }
            None => {
                endings.push("");
                rest = "";
            }
        }
    }
    endings
}

fn restore(endings: &[&'static str], original: &str, edited: &str) -> String {
    let crlf = endings.iter().filter(|e| **e == "\r\n").count();
    let lf = endings.iter().filter(|e| **e == "\n").count();
    let majority = if crlf > lf { "\r\n" } else { "\n" };
    let old: Vec<&str> = original.lines().collect();
    let new: Vec<&str> = edited.lines().collect();
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let trailing = edited.ends_with('\n');
    let mut out = String::with_capacity(edited.len() + new.len());
    for (index, line) in new.iter().enumerate() {
        let kept = if index < prefix {
            endings.get(index).copied()
        } else if index >= new.len() - suffix {
            endings.get(old.len() - (new.len() - index)).copied()
        } else {
            None
        };
        out.push_str(line);
        let last = index + 1 == new.len();
        if last && !trailing {
            break;
        }
        out.push_str(match kept {
            Some(ending) if !ending.is_empty() => ending,
            _ => majority,
        });
    }
    out
}
