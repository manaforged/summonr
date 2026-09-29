use serde_json::Value;

use crate::error::{Error, Result};

use super::{
    ToolOutput, ToolRegistry, nonempty_path, object_bool, object_string, object_usize, parse_args,
};

mod apply_patch;
mod edit;
mod image;
mod list;
mod ops;
mod read;
mod search;
mod spans;
mod write;

pub(crate) use image::{IMAGES_MAX_BYTES, attach, image_type};
use list::{format_directory, format_tree};

impl ToolRegistry {
    pub(super) fn read(&self, arguments: &str) -> Result<ToolOutput> {
        let args: Value = parse_args("read", arguments)?;
        let op = args.get("op").and_then(Value::as_str);
        if let Some(paths) = args
            .get("paths")
            .and_then(Value::as_array)
            .filter(|paths| !paths.is_empty())
        {
            return match op {
                None | Some("file") => self.read_files(&args, paths),
                Some(operation) => Err(Error::Tool(format!(
                    "paths does not apply to read op={operation}"
                ))),
            };
        }
        match op {
            Some(operation) => self.read_op(operation, arguments),
            None => self.read_default(&args, arguments),
        }
    }

    fn read_op(&self, operation: &str, arguments: &str) -> Result<ToolOutput> {
        match operation {
            "file" => self.read_file(arguments),
            "list" => self.list_dir(arguments).map(ToolOutput::ok),
            "search" => self.grep(arguments).map(ToolOutput::ok),
            "image" => self.show_image(arguments),
            "help" => Ok(ToolOutput::ok(super::help::READ.to_string())),
            unknown => Err(Error::Tool(format!("unknown read operation `{unknown}`"))),
        }
    }

    fn read_default(&self, args: &Value, arguments: &str) -> Result<ToolOutput> {
        let pattern = args.get("pattern").and_then(Value::as_str).unwrap_or("");
        let glob = args
            .get("glob")
            .or_else(|| args.get("include"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|glob| !glob.is_empty());
        if !pattern.is_empty() || glob.is_some() {
            return self.grep(arguments).map(ToolOutput::ok);
        }
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .unwrap_or(".");
        let opened = if path == "." {
            self.workspace.clone()
        } else {
            self.resolve_read(path)?
        };
        if opened.is_dir() {
            return self.list_dir(arguments).map(ToolOutput::ok);
        }
        self.read_file(arguments)
    }

    pub(super) async fn edit_file(&self, arguments: &str) -> Result<ToolOutput> {
        let args: Value = parse_args("edit", arguments)?;
        if args.get("op").and_then(Value::as_str) == Some("help") {
            return Ok(ToolOutput::ok(super::help::EDIT.to_string()));
        }
        self.apply_edit("edit", &args).await
    }

    async fn move_file(&self, path: &str, to: &str) -> Result<String> {
        let source = self.resolve_existing(path)?;
        self.check_write(&source)?;
        let dest = self.resolve_write(to)?;
        self.check_write(&dest)?;
        if dest.exists() {
            return Err(Error::Tool(format!("`{to}` already exists")));
        }
        crate::durable::store::rename(&source, &dest)
            .map_err(|error| Error::Tool(format!("cannot move `{path}` to `{to}`: {error}")))?;
        let mut out = format!("moved {path} -> {to}");
        if let Some(check) = self.check_after_edit(&dest).await {
            out.push('\n');
            out.push_str(&check.text);
        }
        Ok(out)
    }

    async fn apply_edit(&self, name: &str, args: &Value) -> Result<ToolOutput> {
        if let Some(patch) = args.get("patch").and_then(Value::as_str) {
            return apply_patch::run(self, patch).await;
        }
        let path = nonempty_path(args, name)?;
        if let Some(to) = edit::move_target(args) {
            let text = self.move_file(path, to).await?;
            return Ok(ToolOutput {
                text,
                success: true,
                mutated: true,
                grant: None,
                images: Vec::new(),
            });
        }
        let dest = self.resolve_existing(path)?;
        self.check_write(&dest)?;
        let raw = std::fs::read_to_string(&dest)
            .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?;
        if raw.is_empty() {
            return Err(Error::Tool(format!(
                "`{path}` is empty — there is no line to anchor an edit; use write"
            )));
        }
        let endings = edit::terminators(&raw);
        let original = raw.replace("\r\n", "\n");
        let (edited, relaxed) = edited_text(&original, args)?;
        let edited = edit::restore(&endings, &original, &edited);
        let preview = self.atomic_write(&dest, path, &edited, edit::uses_tags(args))?;
        let mut out = format!("edited {path}\n{preview}");
        if relaxed {
            out.push_str(
                "\nnote: old matched only with whitespace normalized — check the edit above",
            );
        }
        if let Some(check) = self.check_after_edit(&dest).await {
            out.push('\n');
            out.push_str(&check.text);
        }
        Ok(ToolOutput {
            text: out,
            success: true,
            mutated: true,
            grant: None,
            images: Vec::new(),
        })
    }

    pub(super) async fn write_file(&self, arguments: &str) -> Result<ToolOutput> {
        let args: Value = parse_args("write", arguments)?;
        if let Some(files) = args.get("files").and_then(Value::as_array) {
            return self.write_files(files).await;
        }
        let path = nonempty_path(&args, "write")?;
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Tool("write requires content".to_string()))?;
        let dest = self.resolve_write(path)?;
        self.check_write(&dest)?;
        let note = self.atomic_write(&dest, path, content, write::View::Overwrite)?;
        let mut out = format!("wrote {path} ({} lines, {note})", content.lines().count());
        if let Some(check) = self.check_after_edit(&dest).await {
            out.push('\n');
            out.push_str(&check.text);
        }
        Ok(ToolOutput {
            text: out,
            success: true,
            mutated: true,
            grant: None,
            images: Vec::new(),
        })
    }

    async fn write_files(&self, files: &[Value]) -> Result<ToolOutput> {
        if files.is_empty() {
            return Err(Error::Tool("files is empty".to_string()));
        }
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
            let note = self.stage_write(&dest, &before, content, write::View::Overwrite)?;
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
        for item in &planned {
            if let Some(check) = self.check_after_edit(&item.dest).await {
                out.push_str(&check.text);
                out.push('\n');
            }
        }
        Ok(ToolOutput {
            text: out.trim_end().to_string(),
            success: true,
            mutated: true,
            grant: None,
            images: Vec::new(),
        })
    }

    pub(super) fn list_dir(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("read", arguments)?;
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
        let depth = args
            .get("depth")
            .and_then(Value::as_u64)
            .or_else(|| object_bool(&args, "recursive").then_some(u64::MAX))
            .map(|depth| usize::try_from(depth).unwrap_or(usize::MAX));
        if let Some(depth) = depth {
            return format_tree(path, &dir, depth);
        }
        format_directory(path, &dir)
    }

    pub(super) fn skill(&self, arguments: &str) -> Result<String> {
        let args: Value = parse_args("skill", arguments)?;
        let name = object_string(&args, "name")?;
        crate::skills::load(&self.workspace, name)
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;

fn edited_text(original: &str, args: &Value) -> Result<(String, bool)> {
    let sequence = args.get("op").and_then(Value::as_str) == Some("sequence");
    match args.get("edits").and_then(Value::as_array) {
        Some(ops) if ops.is_empty() => Err(Error::Tool("edits is empty".to_string())),
        Some(ops) if sequence => edit::apply_sequence(original, ops),
        Some(ops) => edit::apply_many(original, ops),
        None => edit::apply_one(original, args)
            .map_err(|error| edit::edit_error_context(error, original, args)),
    }
}
