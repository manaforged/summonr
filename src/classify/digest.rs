use std::collections::BTreeMap;

use crate::tui::ellipsize as clip;

const TASK_CHARS: usize = 400;
const CLOSING_CHARS: usize = 600;
const COMMAND_CHARS: usize = 110;
const MOST_ERRORS: usize = 12;
const MOST_COMMANDS: usize = 25;
const MOST_FILES: usize = 25;

#[derive(Default)]
struct Seen {
    model: String,
    root: String,
    id: String,
    task: String,
    closing: String,
    commands: Vec<String>,
    files: Vec<String>,
    errors: Vec<String>,
    exits: BTreeMap<String, usize>,
    reads: usize,
    edits: usize,
}

pub fn digest(transcript: &str) -> String {
    render(&walk(transcript))
}

fn walk(transcript: &str) -> Seen {
    let mut seen = Seen::default();
    let mut section = String::new();
    let mut body = String::new();
    let mut fenced = false;
    for line in transcript.lines() {
        if let Some(header) = line.strip_prefix("# session ") {
            header_fields(header, &mut seen);
            continue;
        }
        if line.starts_with("## ") {
            close(&section, &body, &mut seen);
            section = line.trim_start_matches("## ").to_string();
            body.clear();
            continue;
        }
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            capture_output(line, &mut seen);
            continue;
        }
        if let Some(call) = line.strip_prefix("- `") {
            capture_call(call, &mut seen);
            continue;
        }
        if body.chars().count() < CLOSING_CHARS * 2 {
            body.push_str(line);
            body.push('\n');
        }
    }
    close(&section, &body, &mut seen);
    seen
}

fn header_fields(header: &str, seen: &mut Seen) {
    let mut parts = header.split(" · ");
    seen.id = parts.next().unwrap_or_default().trim().to_string();
    seen.model = parts.next().unwrap_or_default().trim().to_string();
    seen.root = parts.next().unwrap_or_default().trim().to_string();
}

fn close(section: &str, body: &str, seen: &mut Seen) {
    let text = body.trim();
    if text.is_empty() {
        return;
    }
    if section == "You" && seen.task.is_empty() {
        seen.task = clip(&text.replace('\n', " "), TASK_CHARS);
    }
    if section == "Assistant" {
        seen.closing = clip(&text.replace('\n', " "), CLOSING_CHARS);
    }
}

fn capture_call(call: &str, seen: &mut Seen) {
    let Some((name, rest)) = call.split_once('`') else {
        return;
    };
    match name {
        "run_command" => {
            if let Some(command) = field(rest, "command") {
                seen.commands.push(clip(&command, COMMAND_CHARS));
            }
        }
        "read" => seen.reads += 1,
        "edit" | "write" => {
            seen.edits += 1;
            if let Some(path) = field(rest, "path")
                && !seen.files.contains(&path)
            {
                seen.files.push(path);
            }
        }
        _ => {}
    }
}

fn field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = text.find(&needle)? + needle.len();
    let rest = text.get(start..)?.trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut value = String::new();
    let mut escaped = false;
    for glyph in rest.chars() {
        if escaped {
            value.push(glyph);
            escaped = false;
            continue;
        }
        match glyph {
            '\\' => escaped = true,
            '"' => break,
            _ => value.push(glyph),
        }
    }
    (!value.is_empty()).then_some(value)
}

fn capture_output(line: &str, seen: &mut Seen) {
    let trimmed = line.trim();
    if let Some(code) = trimmed.strip_prefix("exit ") {
        let code = code.split_whitespace().next().unwrap_or(code);
        if code.chars().all(|glyph| glyph.is_ascii_digit()) {
            let seen_before = seen.exits.get(code).copied().unwrap_or_default();
            seen.exits.insert(code.to_string(), seen_before + 1);
            return;
        }
    }
    let lowered = trimmed.to_ascii_lowercase();
    let interesting = lowered.starts_with("error")
        || lowered.starts_with("warning:")
        || lowered.contains("panicked")
        || lowered.contains("test result: failed")
        || lowered.starts_with("failed")
        || lowered.contains("permission denied");
    if interesting
        && seen.errors.len() < MOST_ERRORS
        && !seen.errors.iter().any(|kept| kept == trimmed)
    {
        seen.errors
            .push(clip(&trimmed.replace('\n', " "), COMMAND_CHARS));
    }
}

fn tally(counted: &mut BTreeMap<String, usize>, key: &str) {
    let slot = counted.entry(key.to_string()).or_default();
    let next = *slot + 1;
    counted.insert(key.to_string(), next);
}

fn render(seen: &Seen) -> String {
    let mut counted: BTreeMap<String, usize> = BTreeMap::new();
    for command in &seen.commands {
        tally(&mut counted, command);
    }
    let mut ranked: Vec<(&String, &usize)> = counted.iter().collect();
    ranked.sort_by(|left, right| right.1.cmp(left.1));
    let mut out = format!(
        "session {} \u{b7} {} \u{b7} {}\n\ntask\n{}\n\ncommands: {} run, {} distinct\n",
        seen.id,
        seen.model,
        seen.root,
        seen.task,
        seen.commands.len(),
        counted.len()
    );
    for (command, count) in ranked.iter().take(MOST_COMMANDS) {
        out.push_str(&format!("  {count}x {command}\n"));
    }
    if !seen.exits.is_empty() {
        let shown: Vec<String> = seen
            .exits
            .iter()
            .map(|(code, count)| format!("exit {code}: {count}"))
            .collect();
        out.push_str(&format!("\noutcomes: {}\n", shown.join(", ")));
    }
    if !seen.errors.is_empty() {
        out.push_str("\nerrors and warnings seen\n");
        for error in &seen.errors {
            out.push_str(&format!("  {error}\n"));
        }
    }
    out.push_str(&format!(
        "\nfiles read {} times, edited or written {} times, {} distinct paths\n",
        seen.reads,
        seen.edits,
        seen.files.len()
    ));
    for path in seen.files.iter().take(MOST_FILES) {
        out.push_str(&format!("  {path}\n"));
    }
    out.push_str(&format!("\nclosing statement\n{}\n", seen.closing));
    out
}

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;
