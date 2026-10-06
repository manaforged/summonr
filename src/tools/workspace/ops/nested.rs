use serde_json::Value;

use crate::error::{Error, Result};

use super::super::edit::Patch;
use super::super::object_string;
use super::{
    OP_INSERT_AFTER, OP_REPLACE_TEXT, delete_patch, insert_patch, optional_tag, replace_all,
    replace_patch, replace_text_patch,
};

pub(super) fn nested_patch(text: &str, op: &Value) -> Result<Option<Patch>> {
    if let Some(replace) = op.get("replace") {
        return nested_replace(text, replace).map(Some);
    }
    if let Some(replace) = op.get(OP_REPLACE_TEXT) {
        return nested_replace_text(text, replace).map(Some);
    }
    if let Some(insert) = op.get(OP_INSERT_AFTER) {
        return nested_insert(text, insert).map(Some);
    }
    if let Some(delete) = op.get("delete") {
        return nested_delete(text, delete).map(Some);
    }
    Ok(None)
}

fn nested_replace(text: &str, replace: &Value) -> Result<Patch> {
    if !replace.is_object() {
        return Err(Error::Tool(
            "replace must be an object with tag, new, and optional end_tag; use replace_text with old and new for exact text"
                .to_string(),
        ));
    }
    let tag = object_string(replace, "tag")?;
    let new = replace
        .get("new")
        .or_else(|| replace.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Tool("replace requires new".to_string()))?;
    let end = replace.get("end_tag").and_then(Value::as_str);
    replace_patch(text, tag, end, new)
}

fn nested_replace_text(text: &str, replace: &Value) -> Result<Patch> {
    if !replace.is_object() {
        return Err(Error::Tool(
            "replace_text must be an object with old, new, and optional tag".to_string(),
        ));
    }
    replace_text_patch(
        text,
        optional_tag(replace)?,
        object_string(replace, "old")?,
        object_string(replace, "new")?,
        replace_all(replace),
    )
}

fn nested_insert(text: &str, insert: &Value) -> Result<Patch> {
    if !insert.is_object() {
        return Err(Error::Tool(
            "insert_after must be an object with tag and text".to_string(),
        ));
    }
    let tag = object_string(insert, "tag")?;
    let body = object_string(insert, "text")?;
    insert_patch(text, tag, body)
}

fn nested_delete(text: &str, delete: &Value) -> Result<Patch> {
    if !delete.is_object() {
        return Err(Error::Tool(
            "delete must be an object with tag and optional end_tag".to_string(),
        ));
    }
    let tag = object_string(delete, "tag")?;
    let end = delete.get("end_tag").and_then(Value::as_str);
    delete_patch(text, tag, end)
}
