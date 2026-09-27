use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use grep_regex::{RegexMatcher, RegexMatcherBuilder};
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
        let query = self.query(&args)?;
        let mut files = collect(
            &query.root,
            query.overrides.as_ref(),
            query.glob.is_some(),
            query.types,
            query.no_ignore,
            query.protect,
        );
        files.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let Some(matcher) = &query.matcher else {
            return Ok(file_list(&query.base, &files, query.limit));
        };
        let spec = Spec {
            pattern: &query.pattern,
            limit: query.limit,
            before: query.before,
            after: query.after,
            mode: query.mode,
            multiline: query.multiline,
            tags: query.tags,
        };
        scan::search(&query.base, &files, matcher, &spec)
    }

    fn query(&self, args: &Value) -> Result<Query> {
        let pattern = args.get("pattern").and_then(Value::as_str).unwrap_or("");
        let glob = text_arg(args, "glob", "include");
        if pattern.is_empty() && glob.is_none() {
            return Err(Error::Tool("grep requires pattern or glob".to_string()));
        }
        let multiline = object_bool(args, "multiline") || object_bool(args, "multi_line");
        let (root, base) = self.search_roots(args)?;
        let context = object_usize(args, "context").unwrap_or(0).min(CONTEXT_MAX);
        Ok(Query {
            pattern: pattern.to_string(),
            glob: glob.map(str::to_string),
            overrides: build_overrides(&base, glob, text_arg(args, "exclude", "exclude_pattern"))?,
            types: build_types(args)?,
            matcher: build_matcher(args, pattern, multiline)?,
            root,
            base,
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

    fn search_roots(&self, args: &Value) -> Result<(PathBuf, PathBuf)> {
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
            Ok((root, base))
        } else {
            Ok((root.clone(), root))
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
    let sensitive = args
        .get("caseSensitive")
        .or_else(|| args.get("case_sensitive"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
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

struct Query {
    pattern: String,
    glob: Option<String>,
    overrides: Option<Override>,
    types: Option<ignore::types::Types>,
    root: PathBuf,
    base: PathBuf,
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
