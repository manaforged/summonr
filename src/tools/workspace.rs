use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::hashline::{dirty_span, line_hash, resolve_span, tagged_span, tagged_text};
use crate::provider::{Image, UserInput};

use super::{ToolOutput, ToolRegistry, nonempty_path, object_string, object_usize, parse_args};

const READ_DEFAULT_LIMIT: usize = 120;
const READ_MAX_LIMIT: usize = 400;
const READ_MAX_BYTES: usize = 2 * 1024 * 1024;
const SMALL_FILE_LINES: usize = 80;
const LIST_MAX: usize = 200;
const GREP_DEFAULT: usize = 50;
const GREP_MAX: usize = 200;
const GREP_FILE_MAX: usize = 1024 * 1024;
pub(crate) const IMAGES_MAX_BYTES: usize = 10 * 1024 * 1024;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "__pycache__",
    ".mage",
];

impl ToolRegistry {
    pub(super) fn read_file(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("read_file", arguments)?;
        let path = object_string(&args, "path")?;
        let offset = object_usize(&args, "offset").unwrap_or(1).max(1);
        let limit = object_usize(&args, "limit")
            .unwrap_or(READ_DEFAULT_LIMIT)
            .clamp(1, READ_MAX_LIMIT);
        let canonical = self.resolve_existing(path)?;
        let bytes = std::fs::read(&canonical)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        if bytes.len() > READ_MAX_BYTES {
            return Err(Error::Tool(format!(
                "`{path}` is larger than {READ_MAX_BYTES} bytes — read a smaller range"
            )));
        }
        let text = String::from_utf8(bytes)
            .map_err(|error| Error::Tool(format!("`{path}` is not valid UTF-8: {error}")))?;
        let total = text.lines().count();
        let first = offset.saturating_sub(1);
        if total == 0 {
            return Ok(format!("{path}  {tagged}", tagged = tagged_text(&text)));
        }
        if first >= total {
            return Err(Error::Tool(format!(
                "`{path}` has {total} lines; offset {offset} is past the end"
            )));
        }
        let last = first.saturating_add(limit).saturating_sub(1);
        let body = tagged_span(&text, first, last);
        Ok(format!("{path}  {body}"))
    }

    pub(super) async fn edit_file(&self, arguments: &str) -> Result<ToolOutput> {
        let args: Value = parse_args("edit_file", arguments)?;
        let path = nonempty_path(&args, "edit_file")?;
        let dest = self.resolve_existing(path)?;
        self.policy.write_path(&self.relative(&dest)?)?;
        let original = std::fs::read_to_string(&dest)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        let edited = match args.get("edits").and_then(Value::as_array) {
            Some(ops) if !ops.is_empty() => apply_many(&original, ops)?,
            Some(_) => return Err(Error::Tool("edits is empty".to_string())),
            None => apply_one(&original, &args)?,
        };
        atomic_write(&dest, path, &edited)?;
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        self.policy.write_path(&self.relative(&dest)?)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::Tool(format!("cannot create `{path}`: {error}")))?;
        }
        atomic_write(&dest, path, content)?;
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
            self.resolve_existing(path)?
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
            Some(path) if !path.trim().is_empty() && path != "." => self.resolve_existing(path)?,
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
            grep_walk(
                &root,
                &self.workspace,
                regex.as_ref(),
                glob.as_ref(),
                limit,
                &mut hits,
                &mut scanned,
            )?;
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
    let mut tmp = dest.as_os_str().to_os_string();
    tmp.push(".mage-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, content)
        .map_err(|error| Error::Tool(format!("cannot write `{display}`: {error}")))?;
    std::fs::rename(&tmp, dest)
        .map_err(|error| Error::Tool(format!("cannot replace `{display}`: {error}")))
}

/// Resolve `@path` workspace files into one user prompt.
pub(super) fn attach(workspace: &Path, line: &str) -> Result<UserInput> {
    let mut out = String::new();
    let mut rest = String::new();
    let mut images = Vec::new();
    let mut image_bytes = 0_usize;
    let mut attached = false;
    for word in line.split_whitespace() {
        if let Some(path) = word.strip_prefix('@')
            && !path.is_empty()
        {
            let full = workspace.join(path);
            if full.is_file() {
                let root = workspace
                    .canonicalize()
                    .map_err(|error| Error::Tool(format!("cannot open workspace: {error}")))?;
                let canonical = full.canonicalize()?;
                if !canonical.starts_with(&root) {
                    return Err(Error::Tool(format!(
                        "attachment `{path}` is outside the workspace"
                    )));
                }
                let bytes = std::fs::read(&canonical)?;
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
                continue;
            }
        }
        if !rest.is_empty() {
            rest.push(' ');
        }
        rest.push_str(word);
    }
    let text = if !attached {
        line.to_string()
    } else if rest.is_empty() {
        out
    } else if out.is_empty() {
        rest
    } else {
        format!("{rest}\n{out}")
    };
    Ok(UserInput { text, images })
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

fn grep_walk(
    dir: &Path,
    workspace: &Path,
    regex: Option<&Regex>,
    glob: Option<&Regex>,
    limit: usize,
    hits: &mut Vec<String>,
    scanned: &mut usize,
) -> Result<()> {
    if hits.len() >= limit {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|error| Error::Tool(format!("cannot search `{}`: {error}", dir.display())))?
        .filter_map(std::result::Result::ok)
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if hits.len() >= limit {
            break;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| Error::Tool(format!("cannot search `{}`: {error}", path.display())))?;
        if file_type.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            grep_walk(&path, workspace, regex, glob, limit, hits, scanned)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        grep_file(&path, workspace, regex, glob, limit, hits, scanned)?;
    }
    Ok(())
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

/// One replace / insert_after / delete object applied to `text`.
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
        let tag = object_string(replace, "tag")?;
        let new = replace
            .get("new")
            .or_else(|| replace.get("text"))
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Tool("replace requires new".to_string()))?;
        let end = replace.get("end_tag").and_then(Value::as_str);
        return replace_patch(text, tag, end, new);
    }
    if let Some(insert) = op.get("insert_after") {
        let tag = object_string(insert, "tag")?;
        let body = object_string(insert, "text")?;
        return insert_patch(text, tag, body);
    }
    if let Some(delete) = op.get("delete") {
        let tag = object_string(delete, "tag")?;
        let end = delete.get("end_tag").and_then(Value::as_str);
        return delete_patch(text, tag, end);
    }
    Err(Error::Tool(
        "edit_file requires replace, insert_after, delete, or edits".to_string(),
    ))
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
        let patch = parse_patch(original, op)
            .map_err(|error| Error::Tool(format!("edit {}: {error}", index + 1)))?;
        patches.push(patch);
    }
    apply_patches(original, patches)
}

/// Validate original-file anchor ranges, then apply from bottom to top.
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
    // At a shared boundary, consume the original line before inserting text.
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
