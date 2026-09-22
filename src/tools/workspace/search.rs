use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use grep_regex::RegexMatcherBuilder;
use ignore::WalkBuilder;
use ignore::overrides::{Override, OverrideBuilder};
use ignore::types::TypesBuilder;
use serde_json::Value;

use crate::SKIP_DIRS;
use crate::error::{Error, Result};
use crate::tools::{ToolRegistry, object_bool, object_usize, parse_args};

const GREP_DEFAULT: usize = 50;
const GREP_MAX: usize = 200;
const GREP_FILE_HITS: usize = 20;
const GREP_FILE_MAX: u64 = 1024 * 1024;
const GREP_LINE_MAX: usize = 500;
const CONTEXT_MAX: usize = 10;

impl ToolRegistry {
    pub(super) fn grep(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("grep", arguments)?;
        let pattern = args.get("pattern").and_then(Value::as_str).unwrap_or("");
        let glob = args
            .get("glob")
            .or_else(|| args.get("include"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|glob| !glob.is_empty());
        if pattern.is_empty() && glob.is_none() {
            return Err(Error::Tool("grep requires pattern or glob".to_string()));
        }
        let limit = object_usize(&args, "limit")
            .unwrap_or(GREP_DEFAULT)
            .clamp(1, GREP_MAX);
        let context = object_usize(&args, "context").unwrap_or(0).min(CONTEXT_MAX);
        let before = object_usize(&args, "before")
            .unwrap_or(context)
            .min(CONTEXT_MAX);
        let after = object_usize(&args, "after")
            .unwrap_or(context)
            .min(CONTEXT_MAX);
        let mode = if object_bool(&args, "count") {
            Mode::Count
        } else if object_bool(&args, "files") || object_bool(&args, "files_only") {
            Mode::Files
        } else {
            Mode::Content
        };
        let literal = object_bool(&args, "literal") || object_bool(&args, "fixed_strings");
        let case_sensitive = args
            .get("caseSensitive")
            .or_else(|| args.get("case_sensitive"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let no_ignore = object_bool(&args, "no_ignore");
        let tags = object_bool(&args, "tags");
        let multiline = object_bool(&args, "multiline") || object_bool(&args, "multi_line");
        let root = match args.get("path").and_then(Value::as_str) {
            Some(path) if !path.trim().is_empty() && path != "." => self.resolve_read(path)?,
            Some(_) | None => self.workspace.clone(),
        };
        let matcher = if pattern.is_empty() {
            None
        } else {
            let pattern = if literal {
                regex::escape(pattern)
            } else {
                pattern.to_string()
            };
            Some(
                RegexMatcherBuilder::new()
                    .case_insensitive(!case_sensitive)
                    .multi_line(multiline)
                    .dot_matches_new_line(multiline)
                    .build(&pattern)
                    .map_err(|error| Error::Tool(format!("grep pattern is invalid: {error}")))?,
            )
        };
        let base = self
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| self.workspace.clone());
        let root = root.canonicalize().unwrap_or(root);
        let base = if root.starts_with(&base) {
            base
        } else {
            root.clone()
        };
        let exclude = args
            .get("exclude")
            .or_else(|| args.get("exclude_pattern"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|glob| !glob.is_empty());
        let overrides = build_overrides(&base, glob, exclude)?;
        let types = args
            .get("type")
            .or_else(|| args.get("file_type"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
            .map(|kind| {
                let mut builder = TypesBuilder::new();
                builder.add_defaults();
                builder.select(kind);
                builder
                    .build()
                    .map_err(|error| Error::Tool(format!("grep type is invalid: {error}")))
            })
            .transpose()?;
        let protect = !self.policy.is_unrestricted();
        let mut files = collect(
            &root,
            overrides.as_ref(),
            glob.is_some(),
            types,
            no_ignore,
            protect,
        );
        files.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let Some(matcher) = matcher else {
            return Ok(file_list(&base, &files, limit));
        };
        let spec = Spec {
            pattern,
            limit,
            before,
            after,
            mode,
            multiline,
            tags,
        };
        scan::search(&base, &files, &matcher, &spec)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Content,
    Files,
    Count,
}

struct Spec<'a> {
    pattern: &'a str,
    limit: usize,
    before: usize,
    after: usize,
    mode: Mode,
    multiline: bool,
    tags: bool,
}

fn build_overrides(
    base: &Path,
    glob: Option<&str>,
    exclude: Option<&str>,
) -> Result<Option<Override>> {
    if glob.is_none() && exclude.is_none() {
        return Ok(None);
    }
    let mut builder = OverrideBuilder::new(base);
    if let Some(glob) = glob {
        builder
            .add(glob)
            .map_err(|error| Error::Tool(format!("grep glob is invalid: {error}")))?;
    }
    if let Some(exclude) = exclude {
        builder
            .add(&format!("!{exclude}"))
            .map_err(|error| Error::Tool(format!("grep exclude is invalid: {error}")))?;
    }
    builder
        .build()
        .map(Some)
        .map_err(|error| Error::Tool(format!("grep glob is invalid: {error}")))
}

fn collect(
    root: &Path,
    overrides: Option<&Override>,
    require_match: bool,
    types: Option<ignore::types::Types>,
    no_ignore: bool,
    protect: bool,
) -> Vec<(PathBuf, u64)> {
    if root.is_file() {
        return vec![(root.to_path_buf(), mtime(root))];
    }
    let mut walker = WalkBuilder::new(root);
    walker
        .require_git(false)
        .hidden(false)
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !SKIP_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
        });
    if no_ignore {
        walker
            .ignore(false)
            .git_ignore(false)
            .git_exclude(false)
            .parents(false);
    }
    if let Some(types) = types {
        walker.types(types);
    }
    let mut files = Vec::new();
    for entry in walker.build().filter_map(std::result::Result::ok) {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.into_path();
        if protect && crate::policy::protected_read(&path) {
            continue;
        }
        if let Some(overrides) = overrides {
            let matched = overrides.matched(&path, false);
            if matched.is_ignore() || (require_match && !matched.is_whitelist()) {
                continue;
            }
        }
        let stamp = mtime(&path);
        files.push((path, stamp));
    }
    files
}

fn file_list(base: &Path, files: &[(PathBuf, u64)], limit: usize) -> String {
    if files.is_empty() {
        return "no files match".to_string();
    }
    let shown = files.len().min(limit);
    let mut out = format!("{} files", files.len());
    if files.len() > shown {
        out.push_str(&format!("; showing {shown} most recently modified"));
    }
    for (path, _) in files.iter().take(shown) {
        out.push('\n');
        out.push_str(&relative(base, path));
    }
    out
}

mod scan;

fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn mtime(path: &Path) -> u64 {
    path.metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |span| span.as_secs())
}
