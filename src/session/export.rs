use std::path::Path;

use super::catalog::{name_of, short};
use super::user_text;
use crate::durable::log::{self, Line};
use crate::error::Result;
use crate::provider::ToolCall;

const ARG_WIDTH: usize = 100;

pub fn export(path: &Path) -> Result<String> {
    let mut out = String::new();
    for line in log::read(path)? {
        match line {
            Line::Session {
                id,
                model,
                workspace,
                ..
            } => export_session(&mut out, path, &id.to_string(), &model, &workspace),
            Line::User {
                text,
                source,
                start,
                ..
            } => export_user(&mut out, &text, source.as_deref(), start),
            Line::Assistant {
                text, tool_calls, ..
            } => export_assistant(&mut out, &text, &tool_calls),
            Line::Interrupted { text, .. } => {
                out.push_str(&format!("## Assistant (interrupted)\n\n{text}\n\n"));
            }
            Line::Tool { text, success, .. } => export_tool(&mut out, &text, success),
            Line::Receipt { text, .. } => {
                out.push_str(&format!("> {}\n\n", text.replace('\n', "\n> ")));
            }
            Line::Summary { text, .. } => {
                out.push_str(&format!("## Compacted\n\n{text}\n\n"));
            }
            Line::Clear => out.push_str("---\n\n"),
            Line::Tools { .. }
            | Line::Budget { .. }
            | Line::Workflow { .. }
            | Line::Child { .. }
            | Line::Charge { .. }
            | Line::Rewrite { .. }
            | Line::Close { .. } => {}
        }
    }
    Ok(out)
}

fn export_session(out: &mut String, path: &Path, id: &str, model: &str, workspace: &Path) {
    out.push_str(&format!(
        "# session {} · {model} · {}\n",
        short(id),
        workspace.display()
    ));
    if let Some(name) = name_of(path) {
        out.push_str(&format!("name: {name}\n"));
    }
    out.push('\n');
}

fn export_user(out: &mut String, text: &str, source: Option<&str>, start: bool) {
    if start || source.is_some() {
        let text = user_text(text, source);
        out.push_str(&format!("## You\n\n{text}\n\n"));
    } else {
        for line in text.lines() {
            out.push_str(&format!("> {line}\n"));
        }
        out.push('\n');
    }
}

fn export_assistant(out: &mut String, text: &str, tool_calls: &[ToolCall]) {
    if !text.trim().is_empty() {
        out.push_str(&format!("## Assistant\n\n{text}\n\n"));
    }
    for call in tool_calls {
        let args =
            crate::text::ellipsize(&crate::tui::tool_arg(&call.function.arguments), ARG_WIDTH);
        out.push_str(&format!("- `{}` {args}\n", call.function.name));
    }
    if !tool_calls.is_empty() {
        out.push('\n');
    }
}

fn export_tool(out: &mut String, text: &str, success: bool) {
    let shown: Vec<&str> = text.lines().take(12).collect();
    out.push_str(&format!(
        "```\n{}{}\n```\n{}\n",
        shown.join("\n"),
        if text.lines().count() > 12 {
            "\n…"
        } else {
            ""
        },
        if success { "" } else { "(failed)\n" }
    ));
}
