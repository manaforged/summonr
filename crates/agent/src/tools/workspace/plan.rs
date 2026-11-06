use serde_json::Value;

use super::ops::EditOp;
use super::{edit, write};
use crate::tools::{ToolRegistry, nonempty_path};
use summonr_core::error::{Error, Result};

impl ToolRegistry {
    pub(super) fn plan_edit(
        &self,
        path: &str,
        args: &Value,
    ) -> Result<(write::Change, String, Option<String>)> {
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
        let (preview, patch) = write::preview(&raw, &edited, edit::uses_tags(args));
        let mut out = format!("edited {path}\n{preview}");
        if relaxed {
            out.push_str(
                "\nnote: old matched only with whitespace normalized; check the edit above",
            );
        }
        Ok((write::Change::write(dest, path, edited), out, patch))
    }

    pub(super) fn plan_write(
        &self,
        path: &str,
        content: &str,
    ) -> Result<(write::Change, String, Option<String>)> {
        let dest = self.resolve_write(path)?;
        self.check_write(&dest)?;
        let before = write::current(&dest, path)?;
        let (note, patch) = write::preview(&before, content, write::View::Overwrite);
        let out = format!("wrote {path} ({} lines, {note})", content.lines().count());
        Ok((
            write::Change::write(dest, path, content.to_string()),
            out,
            patch,
        ))
    }

    pub(super) fn stage_files(&self, files: &[Value]) -> Result<(Vec<write::Change>, String)> {
        let mut planned = Vec::with_capacity(files.len());
        let mut out = String::new();
        for file in files {
            let path = nonempty_path(file, "write")?;
            let content = file
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Tool(format!("write requires content for `{path}`")))?;
            let (change, line, _) = self.plan_write(path, content)?;
            out.push_str(&line);
            out.push('\n');
            planned.push(change);
        }
        self.apply_changes(&planned)?;
        Ok((planned, out))
    }
}

fn edited_text(original: &str, args: &Value) -> Result<(String, bool)> {
    let sequence = args.get("op").and_then(Value::as_str) == Some("sequence");
    let flat = ["old", "new"]
        .iter()
        .any(|field| crate::tools::args::named(args, field).is_some());
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
        Some(ops) => {
            let ops: Vec<EditOp> = ops.iter().map(EditOp::parse).collect();
            edit::apply_many(original, &ops, sequence)
        }
        None => edit::apply_one(original, &EditOp::parse(args)),
    }
}

fn blank_edit(op: &Value) -> bool {
    op.as_object().is_none_or(|fields| {
        fields
            .values()
            .all(|value| !crate::tools::args::named_value(value) && value.as_bool() != Some(true))
    })
}
