use std::path::{Path, PathBuf};
use std::sync::Arc;
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
        let (roots, missing) = self.search_roots(&args)?;
        let query = Arc::new(self.query(&args, roots)?);
        let report = walk::run(&query)?;
        let mut out = scan::render(&query, report);
        for line in missing {
            out.push('\n');
            out.push_str(&line);
        }
        Ok(out)
    }

    fn query(&self, args: &Value, roots: Vec<Root>) -> Result<Query> {
        let pattern = args.get("pattern").and_then(Value::as_str).unwrap_or("");
        let glob = text_arg(args, "glob", "include");
        if pattern.is_empty() && glob.is_none() {
            return Err(Error::Tool("search requires pattern or glob".to_string()));
        }
        let multiline = object_bool(args, "multiline") || object_bool(args, "multi_line");
        let context = object_usize(args, "context").unwrap_or(0).min(CONTEXT_MAX);
        let exclude = text_arg(args, "exclude", "exclude_pattern");
        let fold = !sensitive(args);
        let roots = roots
            .into_iter()
            .map(|root| {
                let overrides = build_overrides(&root.base, glob, exclude, fold)?;
                Ok(Root { overrides, ..root })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Query {
            pattern: pattern.to_string(),
            glob: glob.is_some(),
            types: build_types(args)?,
            matcher: build_matcher(args, pattern, multiline)?,
            shown: self.base(),
            roots,
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
        })
    }

    fn base(&self) -> PathBuf {
        self.workspace
            .canonicalize()
            .unwrap_or_else(|_| self.workspace.clone())
    }

    fn search_roots(&self, args: &Value) -> Result<(Vec<Root>, Vec<String>)> {
        let listed: Vec<&str> = args
            .get("paths")
            .and_then(Value::as_array)
            .map(|paths| paths.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if listed.is_empty() {
            let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
            return Ok((vec![self.root(path)?], Vec::new()));
        }
        let mut roots: Vec<Root> = Vec::new();
        let mut missing = Vec::new();
        for path in listed {
            match self.root(path) {
                Ok(root) => roots.push(root),
                Err(error) => missing.push(format!("{path}: {error}")),
            }
        }
        roots.sort_by(|left, right| left.path.cmp(&right.path));
        roots.dedup_by(|later, kept| later.path.starts_with(&kept.path));
        Ok((roots, missing))
    }

    fn root(&self, path: &str) -> Result<Root> {
        let base = self.base();
        let path = if path.trim().is_empty() || path == "." {
            base.clone()
        } else {
            self.resolve_read(path, crate::tools::paths::Shield::Always)?
        };
        let base = if path.starts_with(&base) {
            base
        } else if path.is_file() {
            path.parent()
                .map_or_else(|| path.clone(), Path::to_path_buf)
        } else {
            path.clone()
        };
        Ok(Root {
            path,
            base,
            overrides: None,
        })
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
    let sensitive = sensitive(args);
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
        .map_err(|error| Error::Tool(format!("search pattern is invalid: {error}")))
}

fn sensitive(args: &Value) -> bool {
    args.get("caseSensitive")
        .or_else(|| args.get("case_sensitive"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

struct Root {
    path: PathBuf,
    base: PathBuf,
    overrides: Option<Override>,
}

struct Query {
    pattern: String,
    glob: bool,
    types: Option<ignore::types::Types>,
    roots: Vec<Root>,
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
                .map_err(|error| Error::Tool(format!("search type is invalid: {error}")))
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
        .map_err(|error| Error::Tool(format!("search glob is invalid: {error}")))?;
    if let Some(glob) = glob {
        builder
            .add(glob)
            .map_err(|error| Error::Tool(format!("search glob is invalid: {error}")))?;
    }
    if let Some(exclude) = exclude {
        builder
            .add(&format!("!{exclude}"))
            .map_err(|error| Error::Tool(format!("search exclude is invalid: {error}")))?;
    }
    builder
        .build()
        .map(Some)
        .map_err(|error| Error::Tool(format!("search glob is invalid: {error}")))
}

mod scan;
mod walk;

fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
