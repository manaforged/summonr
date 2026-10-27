use std::path::PathBuf;

use serde_json::Value;

use crate::error::{Error, Result};

use super::{ToolOutput, ToolRegistry, nonempty_path, object_bool, object_string, object_usize};

mod apply_patch;
mod edit;
mod image;
mod list;
mod ops;
mod read;
pub(in crate::tools) mod search;
mod spans;
mod write;

pub(crate) use apply_patch::patch_files;
pub(crate) use image::{IMAGES_MAX_BYTES, attach, image_type};
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
            Some(Some(ReadOp::File)) => self.read_file(args),
            Some(Some(ReadOp::List)) => self.list_dir(args).map(ToolOutput::ok),
            Some(Some(ReadOp::Search)) => self.search_files(args).map(ToolOutput::ok),
            Some(Some(ReadOp::Image)) => self.show_image(args),
            Some(Some(ReadOp::Help)) => Ok(ToolOutput::ok(super::help::read())),
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
        self.read_file(args)
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
            return Ok(ToolOutput::ok(super::help::EDIT.to_string()));
        }
        self.apply_edit("edit", args).await
    }

    async fn move_file(&self, path: &str, to: &str) -> Result<ToolOutput> {
        let source = self.resolve_existing(path)?;
        self.check_write(&source)?;
        let dest = self.resolve_write(to)?;
        self.check_write(&dest)?;
        if dest.exists() {
            return Err(Error::Tool(format!("`{to}` already exists")));
        }
        crate::durable::store::rename(&source, &dest)
            .map_err(|error| Error::Tool(format!("cannot move `{path}` to `{to}`: {error}")))?;
        self.note_wrote(path);
        self.note_wrote(to);
        Ok(self
            .checked((dest, format!("moved {path} -> {to}"), None))
            .await)
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
            return apply_patch::run(self, patch).await;
        }
        let path = nonempty_path(args, name)?;
        if let Some(to) = edit::move_target(args) {
            return self.move_file(path, to).await;
        }
        let edited = super::blocking(|| self.edit_in_place(path, args))?;
        Ok(self.checked(edited).await)
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
        let written = super::blocking(|| self.write_one(path, content))?;
        Ok(self.checked(written).await)
    }

    async fn write_files(&self, files: &[Value]) -> Result<ToolOutput> {
        if files.is_empty() {
            return Err(Error::Tool("files is empty".to_string()));
        }
        let (planned, mut out) = super::blocking(|| self.stage_files(files))?;
        for item in &planned {
            if let Some(check) = self.check_after_edit(&item.dest).await {
                out.push_str(&check.text);
                out.push('\n');
            }
        }
        Ok(ToolOutput::mutated(out.trim_end().to_string(), None))
    }

    fn edit_in_place(&self, path: &str, args: &Value) -> Result<(PathBuf, String, Option<String>)> {
        let dest = self.resolve_existing(path)?;
        self.check_write(&dest)?;
        let raw = std::fs::read_to_string(&dest)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        if raw.is_empty() {
            return Err(Error::Tool(format!(
                "`{path}` is empty: there is no line to anchor an edit; use write"
            )));
        }
        let endings = edit::terminators(&raw);
        let original = raw.replace("\r\n", "\n");
        let (edited, relaxed) = edited_text(&original, args)?;
        let edited = edit::restore(&endings, &original, &edited);
        let written = self.atomic_write(&dest, path, &raw, &edited, edit::uses_tags(args))?;
        let mut out = format!("edited {path}\n{}", written.preview);
        if relaxed {
            out.push_str(
                "\nnote: old matched only with whitespace normalized; check the edit above",
            );
        }
        Ok((dest, out, written.patch))
    }

    fn write_one(&self, path: &str, content: &str) -> Result<(PathBuf, String, Option<String>)> {
        let dest = self.resolve_write(path)?;
        self.check_write(&dest)?;
        let before = write::current(&dest, path)?;
        let written = self.atomic_write(&dest, path, &before, content, write::View::Overwrite)?;
        let out = format!(
            "wrote {path} ({} lines, {})",
            content.lines().count(),
            written.preview
        );
        Ok((dest, out, written.patch))
    }

    fn stage_files(&self, files: &[Value]) -> Result<(Vec<write::Change>, String)> {
        let mut planned = Vec::with_capacity(files.len());
        let mut out = String::new();
        for file in files {
            let path = nonempty_path(file, "write")?.to_string();
            let content = file
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Tool(format!("write requires content for `{path}`")))?;
            let dest = self.resolve_write(&path)?;
            self.check_write(&dest)?;
            let before = write::current(&dest, &path)?;
            let note = write::preview(&before, content, write::View::Overwrite);
            out.push_str(&format!(
                "wrote {path} ({} lines, {note})\n",
                content.lines().count()
            ));
            planned.push(write::Change {
                dest,
                display: path,
                content: Some(content.to_string()),
            });
        }
        write::apply_changes(&planned)?;
        for change in &planned {
            self.note_wrote(&change.display);
        }
        Ok((planned, out))
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
        if let Some(depth) = depth {
            return format_tree(path, &dir, depth);
        }
        format_directory(path, &dir)
    }

    pub(super) fn skill(&self, args: &Value) -> Result<String> {
        crate::skills::load(&self.workspace, object_string(args, "name")?)
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

fn edited_text(original: &str, args: &Value) -> Result<(String, bool)> {
    let sequence = args.get("op").and_then(Value::as_str) == Some("sequence");
    let flat = ["old", "new"]
        .iter()
        .any(|field| super::args::named(args, field).is_some());
    let edits = args
        .get("edits")
        .and_then(Value::as_array)
        .filter(|ops| !(flat && ops.iter().all(blank_edit)));
    if flat && edits.is_some() {
        return Err(Error::Tool(
            "edit got both top-level old/new and edits; send one edit as old/new or every edit in edits".to_string(),
        ));
    }
    match edits {
        Some(ops) if ops.is_empty() => Err(Error::Tool("edits is empty".to_string())),
        Some(ops) if sequence => edit::apply_sequence(original, ops),
        Some(ops) => edit::apply_many(original, ops),
        None => edit::apply_one(original, args)
            .map_err(|error| edit::edit_error_context(error, original, args)),
    }
}

fn blank_edit(op: &Value) -> bool {
    op.as_object().is_none_or(|fields| {
        fields
            .values()
            .all(|value| !super::args::named_value(value) && value.as_bool() != Some(true))
    })
}

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
