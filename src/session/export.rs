use std::path::Path;

use super::catalog::{name_of, short};
use super::user_text;
use crate::durable::log::{self, Line};
use crate::error::Result;

pub fn export(path: &Path) -> Result<String> {
    let mut out = String::new();
    for line in log::read(path)? {
        match line {
            Line::Session {
                id,
                model,
                workspace,
                ..
            } => {
                out.push_str(&format!(
                    "# session {} · {model} · {}\n",
                    short(&id.to_string()),
                    workspace.display()
                ));
                if let Some(name) = name_of(path) {
                    out.push_str(&format!("name: {name}\n"));
                }
                out.push('\n');
            }
            Line::User {
                text,
                source,
                start,
                ..
            } => {
                if start {
                    let text = source.as_deref().unwrap_or_else(|| user_text(&text));
                    out.push_str(&format!("## You\n\n{text}\n\n"));
                } else {
                    out.push_str(&format!("> {}\n\n", text.lines().next().unwrap_or("")));
                }
            }
            Line::Assistant {
                text, tool_calls, ..
            } => {
                if !text.trim().is_empty() {
                    out.push_str(&format!("## Assistant\n\n{text}\n\n"));
                }
                for call in &tool_calls {
                    let args: String = call.function.arguments.chars().take(100).collect();
                    out.push_str(&format!("- `{}` {args}\n", call.function.name));
                }
                if !tool_calls.is_empty() {
                    out.push('\n');
                }
            }
            Line::Interrupted { text, .. } => {
                out.push_str(&format!("## Assistant (interrupted)\n\n{text}\n\n"));
            }
            Line::Tool { text, success, .. } => {
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
