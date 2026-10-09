use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::types::TypesBuilder;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::tools::{ToolRegistry, object_bool, object_usize, parse_args};

pub(in crate::tools) const GREP_DEFAULT: usize = 50;
pub(in crate::tools) const GREP_MAX: usize = 200;
const GREP_FILE_HITS: usize = 20;
const GREP_FILE_MAX: u64 = 1024 * 1024;
const GREP_LINE_MAX: usize = 500;
pub(in crate::tools) const CONTEXT_MAX: usize = 10;
const OUTPUT_MAX: usize = 50 * 1024;

impl ToolRegistry {
    pub(in crate::tools) fn search_files(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("search", arguments)?;
        let (roots, missing) = self.search_roots(&args)?;
        let query = Arc::new(self.query(&args, roots)?);
        let started = Instant::now();
        let report = walk::run(&query)?;
        let slow = started.elapsed().as_millis()
            >= u128::from(crate::limits::limits().search_index_after_ms);
        for root in query
            .roots
            .iter()
            .filter(|root| query.indexable(&root.path))
        {
            index::refresh(&root.path, slow);
        }
        let out = render::render(&query, report);
        Ok(if missing.is_empty() {
            out
        } else {
            format!("not searched: {}\n{out}", missing.join("; "))
        })
    }

    fn query(&self, args: &Value, roots: Vec<Root>) -> Result<Query> {
        let literal = object_bool(args, "literal") || object_bool(args, "fixed_strings");
        let pattern = patterns(args, literal);
        let glob = text_arg(args, "glob", "include").map(widened);
        let name = filter::name_words(args);
        if pattern.is_empty() && glob.is_none() && name.is_empty() {
            return Err(Error::Tool(
                "search requires pattern, name, or glob".to_string(),
            ));
        }
        let multiline = object_bool(args, "multiline") || object_bool(args, "multi_line");
        let context = object_usize(args, "context").unwrap_or(0).min(CONTEXT_MAX);
        let exclude = text_arg(args, "exclude", "exclude_pattern");
        let fold = !sensitive(args);
        let roots = roots
            .into_iter()
            .map(|root| {
                let overrides = build_overrides(&root.base, glob.as_deref(), exclude, fold)?;
                Ok(Root { overrides, ..root })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Query {
            matcher: build_matcher(args, &pattern, multiline)?,
            pattern,
            glob: glob.is_some(),
            name,
            newer: filter::newer(args)?,
            group: filter::grouped(args)?,
            offset: object_usize(args, "offset").unwrap_or(0),
            types: build_types(args)?,
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
            mode: Mode::of(args)?,
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
            .or_else(|| args.get("path"))
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
        roots.dedup_by(|later, kept| later.path == kept.path);
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

fn patterns(args: &Value, literal: bool) -> String {
    let quote = |text: &str| {
        if literal {
            regex::escape(text)
        } else {
            text.to_string()
        }
    };
    match args.get("pattern") {
        Some(Value::String(text)) => quote(text),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| format!("(?:{})", quote(text)))
            .collect::<Vec<_>>()
            .join("|"),
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_)) | None => {
            String::new()
        }
    }
}

fn widened(glob: &str) -> String {
    if glob.contains(['*', '?', '[', '{', '/']) {
        glob.to_string()
    } else {
        format!("*{glob}*")
    }
}

fn build_matcher(args: &Value, pattern: &str, multiline: bool) -> Result<Option<RegexMatcher>> {
    if pattern.is_empty() {
        return Ok(None);
    }
    RegexMatcherBuilder::new()
        .case_insensitive(!sensitive(args))
        .word(object_bool(args, "word"))
        .multi_line(multiline)
        .dot_matches_new_line(multiline)
        .build(pattern)
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
    name: Vec<String>,
    newer: Option<u64>,
    group: bool,
    offset: usize,
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

impl Query {
    fn indexable(&self, root: &Path) -> bool {
        (self.matcher.is_none() || self.glob || !self.name.is_empty())
            && !root.starts_with(&self.shown)
    }
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

const OUTPUTS: &[(&str, Mode)] = &[
    ("lines", Mode::Content),
    ("content", Mode::Content),
    ("files", Mode::Files),
    ("count", Mode::Count),
    ("folders", Mode::Files),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Content,
    Files,
    Count,
}

impl Mode {
    fn of(args: &Value) -> Result<Self> {
        if let Some(name) = args.get("output").and_then(Value::as_str) {
            return OUTPUTS
                .iter()
                .find(|(output, _)| *output == name)
                .map(|(_, mode)| *mode)
                .ok_or_else(|| {
                    Error::Tool(format!(
                        "search output `{name}` is invalid: use lines, files, count, or folders"
                    ))
                });
        }
        Ok(if object_bool(args, "count") {
            Self::Count
        } else if object_bool(args, "files") || object_bool(args, "files_only") {
            Self::Files
        } else {
            Self::Content
        })
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

mod filter;
pub(in crate::tools) mod index;
mod render;
mod scan;
mod walk;

fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
