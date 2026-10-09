use std::path::{Path, PathBuf};
use std::time::Instant;

use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::types::TypesBuilder;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::tools::{ToolRegistry, object_bool, object_usize, parse_args};

const GREP_DEFAULT: usize = 50;
const GREP_MAX: usize = 200;
const GREP_FILE_HITS: usize = 20;
const GREP_FILE_MAX: u64 = 1024 * 1024;
const GREP_LINE_MAX: usize = 500;
const CONTEXT_MAX: usize = 10;

impl ToolRegistry {
    pub(in crate::tools) fn search_files(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("search", arguments)?;
        match args
            .get("paths")
            .and_then(Value::as_array)
            .filter(|paths| !paths.is_empty())
        {
            Some(paths) => Ok(self.search_paths(&args, paths)),
            None => self.grep(arguments),
        }
    }

    pub(super) fn search_paths(&self, args: &Value, paths: &[Value]) -> String {
        let mut found = Vec::new();
        for path in paths.iter().filter_map(Value::as_str) {
            let mut one = args.clone();
            if let Some(fields) = one.as_object_mut() {
                fields.remove("paths");
                fields.insert("path".to_string(), Value::from(path));
            }
            found.push(match self.grep(&one.to_string()) {
                Ok(hits) => hits,
                Err(error) => format!("{path}: {error}"),
            });
        }
        found.join("\n")
    }

    pub(super) fn grep(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("grep", arguments)?;
        let query = self.query(&args)?;
        let (mut files, stopped) = collect(&query);
        files.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let Some(matcher) = &query.matcher else {
            return Ok(file_list(&query.shown, &files, query.limit, stopped));
        };
        let spec = Spec {
            pattern: &query.pattern,
            limit: query.limit,
            before: query.before,
            after: query.after,
            mode: query.mode,
            multiline: query.multiline,
            tags: query.tags,
            deadline: query.deadline,
        };
        scan::search(&query.shown, &files, matcher, &spec, stopped)
    }

    fn query(&self, args: &Value) -> Result<Query> {
        let pattern = args.get("pattern").and_then(Value::as_str).unwrap_or("");
        let glob = text_arg(args, "glob", "include");
        if pattern.is_empty() && glob.is_none() {
            return Err(Error::Tool("grep requires pattern or glob".to_string()));
        }
        let multiline = object_bool(args, "multiline") || object_bool(args, "multi_line");
        let (root, base, inside) = self.search_roots(args)?;
        let context = object_usize(args, "context").unwrap_or(0).min(CONTEXT_MAX);
        let exclude = text_arg(args, "exclude", "exclude_pattern");
        let fold = !sensitive(args, glob.unwrap_or_default());
        Ok(Query {
            pattern: pattern.to_string(),
            glob: glob.map(str::to_string),
            overrides: build_overrides(&base, glob, exclude, fold)?,
            types: build_types(args)?,
            matcher: build_matcher(args, pattern, multiline)?,
            shown: if inside { base } else { PathBuf::new() },
            root,
            depth: object_usize(args, "depth"),
            deadline: Instant::now() + crate::limits::limits().search(),
            limit: object_usize(args, "limit")
                .unwrap_or(GREP_DEFAULT)
                .clamp(1, GREP_MAX),
            before: object_usize(args, "before")
                .unwrap_or(context)
                .min(CONTEXT_MAX),
            after: object_usize(args, "after")
                .unwrap_or(context)
                .min(CONTEXT_MAX),
            mode: Mode::of(args),
            multiline,
            tags: object_bool(args, "tags"),
            no_ignore: object_bool(args, "no_ignore"),
            protect: !self.policy.is_unrestricted(),
        })
    }

    fn search_roots(&self, args: &Value) -> Result<(PathBuf, PathBuf, bool)> {
        let root = match args.get("path").and_then(Value::as_str) {
            Some(path) if !path.trim().is_empty() && path != "." => self.resolve_read(path)?,
            Some(_) | None => self.workspace.clone(),
        };
        let base = self
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| self.workspace.clone());
        let root = root.canonicalize().unwrap_or(root);
        if root.starts_with(&base) {
            Ok((root, base, true))
        } else {
            let base = if root.is_file() {
                root.parent()
                    .map_or_else(|| root.clone(), Path::to_path_buf)
            } else {
                root.clone()
            };
            Ok((root, base, false))
        }
    }
}

fn text_arg<'a>(args: &'a Value, name: &str, alias: &str) -> Option<&'a str> {
    args.get(name)
        .or_else(|| args.get(alias))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn build_matcher(args: &Value, pattern: &str, multiline: bool) -> Result<Option<RegexMatcher>> {
    if pattern.is_empty() {
        return Ok(None);
    }
    let literal = object_bool(args, "literal") || object_bool(args, "fixed_strings");
    let sensitive = sensitive(args, pattern);
    let pattern = if literal {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };
    RegexMatcherBuilder::new()
        .case_insensitive(!sensitive)
        .multi_line(multiline)
        .dot_matches_new_line(multiline)
        .build(&pattern)
        .map(Some)
        .map_err(|error| Error::Tool(format!("grep pattern is invalid: {error}")))
}

fn sensitive(args: &Value, text: &str) -> bool {
    args.get("caseSensitive")
        .or_else(|| args.get("case_sensitive"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| text.chars().any(char::is_uppercase))
}

struct Query {
    pattern: String,
    glob: Option<String>,
    overrides: Option<Override>,
    types: Option<ignore::types::Types>,
    root: PathBuf,
    shown: PathBuf,
    depth: Option<usize>,
    deadline: Instant,
    matcher: Option<RegexMatcher>,
    limit: usize,
    before: usize,
    after: usize,
    mode: Mode,
    multiline: bool,
    tags: bool,
    no_ignore: bool,
    protect: bool,
}

fn build_types(args: &Value) -> Result<Option<ignore::types::Types>> {
    args.get("type")
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
        .transpose()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Content,
    Files,
    Count,
}

impl Mode {
    fn of(args: &Value) -> Self {
        if object_bool(args, "count") {
            Self::Count
        } else if object_bool(args, "files") || object_bool(args, "files_only") {
            Self::Files
        } else {
            Self::Content
        }
    }
}

struct Spec<'a> {
    pattern: &'a str,
    limit: usize,
    before: usize,
    after: usize,
    mode: Mode,
    multiline: bool,
    tags: bool,
    deadline: Instant,
}

fn build_overrides(
    base: &Path,
    glob: Option<&str>,
    exclude: Option<&str>,
    fold: bool,
) -> Result<Option<Override>> {
    if glob.is_none() && exclude.is_none() {
        return Ok(None);
    }
    let mut builder = OverrideBuilder::new(base);
    builder
        .case_insensitive(fold)
        .map_err(|error| Error::Tool(format!("grep glob is invalid: {error}")))?;
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

fn file_list(base: &Path, files: &[(PathBuf, u64, u64)], limit: usize, stopped: bool) -> String {
    if files.is_empty() {
        let mut out = "no files match".to_string();
        out.push_str(&stopped_note(stopped));
        return out;
    }
    let shown = files.len().min(limit);
    let mut out = format!("{} files", files.len());
    if files.len() > shown {
        out.push_str(&format!("; showing {shown} most recently modified"));
    }
    out.push_str(&stopped_note(stopped));
    for (path, ..) in files.iter().take(shown) {
        out.push('\n');
        out.push_str(&relative(base, path));
    }
    out
}

mod scan;
mod walk;

use walk::collect;

fn stopped_note(stopped: bool) -> String {
    if !stopped {
        return String::new();
    }
    format!(
        "; stopped after {}s: narrow path, glob, or depth for the rest",
        crate::limits::limits().search_secs
    )
}

fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
