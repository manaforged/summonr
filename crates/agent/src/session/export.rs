use std::path::Path;

use super::catalog::{name_of, short};
use super::user_text;
use crate::durable::log::{self, Line};
use summonr_core::error::Result;
use summonr_types::ToolCall;

const ARG_WIDTH: usize = 100;

/// Stream complete journal and mail-audit records without dropping provider fields.
pub fn export_json(path: &Path, output: &mut impl std::io::Write) -> Result<()> {
    log::readable(path)?;
    let id = log::parse_path(path)
        .ok_or_else(|| crate::Error::Config("session path has no id".to_string()))?
        .to_string();
    let mut records = audit_records(path, &id, "session", output)?;
    let home = log::home_of(path)
        .ok_or_else(|| crate::Error::Config("session path has no home".to_string()))?;
    let mail = crate::mail::files::audit_path(&home.join("mail"), &id);
    if mail.exists() {
        records += audit_records(&mail, &id, "mail", output)?;
    }
    serde_json::to_writer(
        &mut *output,
        &serde_json::json!({
            "schema": 1, "session": id, "source": "export", "sequence": 1,
            "record": {"kind": "complete_prefix", "records": records},
        }),
    )?;
    output.write_all(b"\n")?;
    Ok(())
}

fn audit_records(
    path: &Path,
    session: &str,
    source: &str,
    output: &mut impl std::io::Write,
) -> Result<u64> {
    use std::io::BufRead as _;
    #[derive(serde::Serialize)]
    struct Record<'a> {
        schema: u8,
        session: &'a str,
        source: &'a str,
        sequence: u64,
        record: &'a serde_json::value::RawValue,
    }
    let mut reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut bytes = Vec::new();
    let mut sequence = 0;
    while reader.read_until(b'\n', &mut bytes)? != 0 {
        if bytes.last() != Some(&b'\n') {
            break;
        }
        sequence += 1;
        let record = serde_json::from_slice(&bytes)?;
        serde_json::to_writer(
            &mut *output,
            &Record {
                schema: 1,
                session,
                source,
                sequence,
                record,
            },
        )?;
        output.write_all(b"\n")?;
        bytes.clear();
    }
    Ok(sequence)
}

pub fn export(path: &Path) -> Result<String> {
    log::readable(path)?;
    let mut out = String::new();
    for line in log::records(path, 0) {
        match line? {
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
            Line::Agent { .. }
            | Line::Topic { .. }
            | Line::Skills { .. }
            | Line::Tools { .. }
            | Line::Work { .. }
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
        let args = crate::display::ellipsize(
            &crate::display::tool_arg(&call.function.arguments),
            ARG_WIDTH,
        );
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

const DECORATION: &str = "session name you assistant interrupted compacted failed in refs";
const RAW_ONLY: [&[u8]; 4] = [
    b"\\u",
    b"\"kind\":\"receipt\"",
    "\u{212a}".as_bytes(),
    "\u{130}".as_bytes(),
];

impl super::SessionInfo {
    #[must_use]
    pub fn may_contain(&self, needle: &str) -> bool {
        if needle.is_empty()
            || !needle.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || DECORATION.contains(needle)
        {
            return true;
        }
        let Ok(mut bytes) = std::fs::read(&self.path) else {
            return true;
        };
        bytes.make_ascii_lowercase();
        let has = |part: &[u8]| bytes.windows(part.len()).any(|window| window == part);
        has(needle.as_bytes())
            || RAW_ONLY.into_iter().any(has)
            || name_of(&self.path).is_some_and(|name| name.to_lowercase().contains(needle))
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
