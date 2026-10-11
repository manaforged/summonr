use serde_json::Value;

use crate::provider::Image;
use crate::tools::{IMAGES_MAX_BYTES, ToolOutput};

pub(super) fn output(mut result: Value, mutated: bool) -> ToolOutput {
    let success = !result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let items = if let Some(Value::Array(items)) = result.get_mut("content").map(Value::take) {
        items
    } else {
        Vec::new()
    };
    let mut text = String::new();
    let mut parts = 0;
    let mut images = Vec::new();
    let mut image_bytes = 0;
    for mut item in items {
        let kind = item.get_mut("type").map(Value::take);
        let part = match kind.as_ref().and_then(Value::as_str) {
            Some("text") => owned(item.get_mut("text")),
            Some("resource") => owned(item.pointer_mut("/resource/text"))
                .or_else(|| owned(item.pointer_mut("/resource/uri"))),
            Some("resource_link") => owned(item.get_mut("uri")),
            Some("image") => image(&mut item, &mut image_bytes).map_or_else(
                || Some("[image content omitted]".to_string()),
                |kept| {
                    images.push(kept);
                    None
                },
            ),
            Some(kind) => Some(format!("[{kind} content omitted]")),
            None => None,
        };
        if let Some(part) = part {
            append(&mut text, part);
            parts += 1;
        }
    }
    if parts == 0
        && let Some(structured) = result.get("structuredContent")
    {
        text = structured.to_string();
    }
    ToolOutput {
        text,
        success,
        mutated,
        grant: None,
        images,
        patch: None,
    }
}

fn owned(slot: Option<&mut Value>) -> Option<String> {
    let Some(Value::String(text)) = slot.map(Value::take) else {
        return None;
    };
    Some(text)
}

fn append(text: &mut String, part: String) {
    if text.is_empty() {
        *text = part;
        return;
    }
    text.push('\n');
    text.push_str(&part);
}

fn image(item: &mut Value, used: &mut usize) -> Option<Image> {
    let media_type = owned(item.get_mut("mimeType"))?;
    let data = owned(item.get_mut("data"))?;
    let bytes = data.len() / 4 * 3;
    if *used + bytes > IMAGES_MAX_BYTES {
        return None;
    }
    *used += bytes;
    let raw = Image {
        media_type: media_type.clone(),
        data,
    }
    .bytes()
    .ok()?;
    (crate::tools::image_type(&raw) == Some(media_type.as_str()))
        .then(|| Image::fit(&raw).ok())
        .flatten()
}
