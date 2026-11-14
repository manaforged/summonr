use std::path::PathBuf;

use serde_json::Value;

use summonr_core::error::{Error, Result};

use super::{ToolOutput, ToolRegistry, nonempty_path, object_bool, object_string, object_usize};

mod apply_patch;
pub(in crate::tools) mod edit;
mod image;
mod list;
mod ops;
mod plan;
mod read;
pub(in crate::tools) mod search;
mod spans;
pub(in crate::tools) mod write;

pub(crate) use apply_patch::patch_files;
pub(crate) use image::{IMAGES_MAX_BYTES, attach, attachments, image_type, joined};
use list::{format_directory, format_tree};

impl ToolRegistry {
    pub(super) fn read(&self, args: &Value) -> Result<ToolOutput> {
        let name = super::args::named(args, "op").unwrap_or_default();
        let op = read_op(args, name);
        if let Some(paths) = args
            .get("paths")
            .and_then(Value::as_array)
            .filter(|paths| !paths.is_empty())
        {
            return match op {
                None if args.get("pattern").is_none() => self.read_files(args, paths),
                Some(Some(ReadOp::File)) => self.read_files(args, paths),
                None | Some(Some(ReadOp::Search)) => self.search_files(args).map(ToolOutput::ok),
                Some(_) => Err(Error::Tool(format!(
                    "paths does not apply to read op={name}"
                ))),
            };
        }
        match op {
            None => self.read_default(args),
            Some(Some(ReadOp::File)) => self.read_file(args, read::READ_PAGE_BYTES),
            Some(Some(ReadOp::List)) => self.list_dir(args).map(ToolOutput::ok),
            Some(Some(ReadOp::Search)) => self.search_files(args).map(ToolOutput::ok),
            Some(Some(ReadOp::Image)) => self.show_image(object_string(args, "path")?),
            Some(Some(ReadOp::Help)) => {
                Ok(ToolOutput::ok(super::catalog::help(super::Builtin::Read)))
            }
            Some(None) => Err(Error::Tool(format!("unknown read operation `{name}`"))),
        }
    }

    fn read_default(&self, args: &Value) -> Result<ToolOutput> {
        let pattern = args.get("pattern").and_then(Value::as_str).unwrap_or("");
        let glob = args
            .get("glob")
            .or_else(|| args.get("include"))
            .is_some_and(super::args::named_value);
        if !pattern.is_empty() || glob {
            return self.search_files(args).map(ToolOutput::ok);
        }
        if self.opened_path(args)?.1.is_dir() {
            return self.list_dir(args).map(ToolOutput::ok);
        }
        self.read_file(args, read::READ_PAGE_BYTES)
    }

    fn opened_path<'a>(&self, args: &'a Value) -> Result<(&'a str, PathBuf)> {
        let path = super::args::named(args, "path").unwrap_or(".");
        if path == "." {
            return Ok((path, self.workspace.clone()));
        }
        let opened = self.resolve_read(path, crate::tools::paths::Shield::Contained)?;
        Ok((path, opened))
    }

    pub(super) async fn edit_file(&self, args: &Value) -> Result<ToolOutput> {
        if super::args::named(args, "op") == Some("help")
            && super::args::named(args, "path").is_none()
        {
            return Ok(ToolOutput::ok(super::catalog::help(super::Builtin::Edit)));
        }
        self.apply_edit("edit", args).await
    }

    fn plan_move(&self, path: &str, to: &str) -> Result<write::Change> {
        let source = self.resolve_existing(path)?;
        self.check_write(&source)?;
        let dest = self.resolve_write(to)?;
        self.check_write(&dest)?;
        if dest.exists() {
            return Err(Error::Tool(format!("`{to}` already exists")));
        }
        Ok(write::Change {
            dest: source,
            display: path.to_string(),
            action: write::Action::Move {
                to: dest.clone(),
                display: to.to_string(),
            },
        })
    }

    async fn checked(
        &self,
        (dest, mut out, patch): (PathBuf, String, Option<String>),
    ) -> ToolOutput {
        if let Some(check) = self.check_after_edit(&dest).await {
            out.push('\n');
            out.push_str(&check.text);
        }
        ToolOutput::mutated(out, patch)
    }

    async fn apply_edit(&self, name: &str, args: &Value) -> Result<ToolOutput> {
        if let Some(patch) = args.get("patch").and_then(Value::as_str) {
            if args.get("preview").is_some() {
                return Err(Error::Tool(
                    "patch does not accept preview; use old/new or files for a preview".to_string(),
                ));
            }
            return apply_patch::run(self, patch).await;
        }
        let files = match args.get("files") {
            None => std::slice::from_ref(args),
            Some(Value::Array(files)) if !files.is_empty() => {
                if super::args::named(args, "path").is_some() {
                    return Err(Error::Tool("send path or files, not both".to_string()));
                }
                files.as_slice()
            }
            Some(_) => return Err(Error::Tool("files must be a nonempty array".to_string())),
        };
        let preview = match args.get("preview") {
            None => false,
            Some(Value::Bool(preview)) => *preview,
            Some(_) => return Err(Error::Tool("preview must be a boolean".to_string())),
        };
        let (changes, mut out, patches) = super::blocking(|| {
            let mut changes = Vec::new();
            let mut paths = std::collections::BTreeSet::new();
            let mut reports = Vec::new();
            let mut patches = Vec::new();
            for file in files {
                if args.get("files").is_some() && file.get("expected_matches").is_none() {
                    return Err(Error::Tool(
                        "every file replacement requires expected_matches".to_string(),
                    ));
                }
                let path = nonempty_path(file, name)?;
                let (change, report, patch) = match edit::move_target(file) {
                    Some(to) => (
                        self.plan_move(path, to)?,
                        format!("moved {path} -> {to}"),
                        None,
                    ),
                    None => self.plan_edit(path, file)?,
                };
                let mut touched = vec![change.dest.as_path()];
                if let write::Action::Move { to, .. } = &change.action {
                    touched.push(to.as_path());
                }
                for path in touched {
                    if !paths.insert(path.to_path_buf()) {
                        return Err(Error::Tool(format!(
                            "{} occurs more than once in the edit batch",
                            path.display()
                        )));
                    }
                }
                reports.push(if preview {
                    format!("{report}\n{}", patch.as_deref().unwrap_or(""))
                } else {
                    report
                });
                if let Some(patch) = patch {
                    patches.push(patch);
                }
                changes.push(change);
            }
            if !preview {
                self.apply_changes(&changes)?;
            }
            Ok((changes, reports.join("\n\n"), patches))
        })?;
        if preview {
            return Ok(ToolOutput::ok(format!(
                "Preview only; no files changed.\n{out}"
            )));
        }
        let dests: Vec<_> = changes
            .into_iter()
            .map(|change| match change.action {
                write::Action::Move { to, .. } => to,
                write::Action::Write(_) | write::Action::Delete => change.dest,
            })
            .collect();
        let checks =
            futures::future::join_all(dests.iter().map(|dest| self.check_after_edit(dest))).await;
        for check in checks.into_iter().flatten() {
            out.push('\n');
            out.push_str(&check.text);
        }
        Ok(ToolOutput::mutated(
            out,
            (!patches.is_empty()).then(|| patches.join("\n")),
        ))
    }

    pub(super) async fn write_file(&self, args: &Value) -> Result<ToolOutput> {
        if let Some(files) = args.get("files").and_then(Value::as_array) {
            return self.write_files(files).await;
        }
        let path = nonempty_path(args, "write")?;
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Tool("write requires content".to_string()))?;
        let written = super::blocking(|| {
            let (change, out, patch) = self.plan_write(path, content)?;
            self.apply_changes(std::slice::from_ref(&change))?;
            Ok::<_, Error>((change.dest, out, patch))
        })?;
        Ok(self.checked(written).await)
    }

    async fn write_files(&self, files: &[Value]) -> Result<ToolOutput> {
        if files.is_empty() {
            return Err(Error::Tool("files is empty".to_string()));
        }
        let (planned, mut out) = super::blocking(|| self.stage_files(files))?;
        let checks =
            futures::future::join_all(planned.iter().map(|item| self.check_after_edit(&item.dest)))
                .await;
        for check in checks.into_iter().flatten() {
            out.push_str(&check.text);
            out.push('\n');
        }
        Ok(ToolOutput::mutated(
            out.trim_end().to_string(),
            write::patch_of(&planned),
        ))
    }

    pub(super) fn list_dir(&self, args: &Value) -> Result<String> {
        let (path, dir) = self.opened_path(args)?;
        if !dir.is_dir() {
            return Err(Error::Tool(format!("`{path}` is not a directory")));
        }
        let depth = args
            .get("depth")
            .and_then(Value::as_u64)
            .or_else(|| object_bool(args, "recursive").then_some(u64::MAX))
            .map(|depth| usize::try_from(depth).unwrap_or(usize::MAX));
        let base = summonr_core::walk::Walk::tool(object_bool(args, "no_ignore"));
        if let Some(depth) = depth {
            return format_tree(path, &dir, depth, base);
        }
        format_directory(path, &dir, base)
    }

    pub(super) async fn skill(&self, args: &Value) -> Result<ToolOutput> {
        let body = crate::skills::load(
            &self.workspace,
            self.skill_set.as_ref(),
            object_string(args, "name")?,
            self.session_id().as_deref(),
        )?;
        let input = self.inject(&body).await?;
        Ok(ToolOutput {
            images: input.images,
            ..ToolOutput::ok(input.text)
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadOp {
    File,
    List,
    Search,
    Image,
    Help,
}

const READ_OPS: &[(&str, ReadOp)] = &[
    ("file", ReadOp::File),
    ("list", ReadOp::List),
    ("search", ReadOp::Search),
    ("image", ReadOp::Image),
    ("help", ReadOp::Help),
];

impl ReadOp {
    fn of(name: &str) -> Option<Self> {
        READ_OPS
            .iter()
            .find(|(op, _)| *op == name)
            .map(|(_, op)| *op)
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;

fn read_op(args: &Value, name: &str) -> Option<Option<ReadOp>> {
    let target = ["path", "pattern"]
        .iter()
        .any(|field| super::args::named(args, field).is_some())
        || args
            .get("paths")
            .and_then(Value::as_array)
            .is_some_and(|paths| paths.iter().any(super::args::named_value));
    let op = (!name.is_empty()).then(|| ReadOp::of(name));
    op.filter(|op| !(*op == Some(ReadOp::Help) && target))
}
