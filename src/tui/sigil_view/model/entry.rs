use super::*;

#[derive(Clone, Debug)]
pub(in crate::tui::sigil_view) enum Entry {
    User(String),
    Thinking(String),
    Receipt(String),
    Assistant {
        text: String,
        done: bool,
    },
    Tool {
        id: String,
        name: String,
        arg: String,
        state: ToolState,
        output: String,
        expanded: bool,
    },
    ToolGroup {
        name: String,
        calls: Vec<Call>,
    },
    Verify {
        command: String,
        passed: bool,
    },
    Mail {
        from: String,
        text: String,
    },
    Aborted(String),
    System(String),
    Error(String),
    Chart(Vec<Round>),
    Image {
        source: ImageSource,
        bytes: std::rc::Rc<[u8]>,
        label: String,
    },
}

#[derive(Clone, Debug)]
pub(in crate::tui::sigil_view) struct Call {
    pub(in crate::tui::sigil_view) id: String,
    pub(in crate::tui::sigil_view) arg: String,
    pub(in crate::tui::sigil_view) state: ToolState,
    pub(in crate::tui::sigil_view) output: String,
    pub(in crate::tui::sigil_view) expanded: bool,
}

impl Call {
    pub(in crate::tui::sigil_view) fn new(
        id: String,
        arg: String,
        state: ToolState,
        output: String,
    ) -> Self {
        Self {
            id,
            arg,
            state,
            output,
            expanded: false,
        }
    }
}

pub(in crate::tui::sigil_view) fn entry_markdown(entry: &Entry) -> String {
    match entry {
        Entry::User(text) => format!("### You\n{text}"),
        Entry::Thinking(text) => format!("> **Thinking**\n> {}", text.replace('\n', "\n> ")),
        Entry::Receipt(text) => text.clone(),
        Entry::Assistant { text, .. } => text.clone(),
        Entry::Tool {
            name,
            arg,
            state,
            output,
            ..
        } => tool_markdown(name, arg, state, output),
        Entry::ToolGroup { name, calls } => calls
            .iter()
            .map(|call| tool_markdown(name, &call.arg, &call.state, &call.output))
            .collect::<Vec<_>>()
            .join("\n"),
        Entry::Verify { command, passed } => {
            format!("{} **verify** `{command}`", if *passed { "✓" } else { "✗" })
        }
        Entry::Mail { from, text } => {
            format!("> **mail from {from}**\n> {}", text.replace('\n', "\n> "))
        }
        Entry::Aborted(reason) => format!("⚠ **aborted:** {reason}"),
        Entry::System(text) => format!("> {text}"),
        Entry::Error(text) => format!("**Error:** {text}"),
        Entry::Chart(rounds) => super::app::presentation::stats(rounds),
        Entry::Image { label, .. } => format!("   {label}"),
    }
}

pub(in crate::tui::sigil_view) fn linked(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("![") {
        let after = &rest[start + 2..];
        let Some(open) = after.find("](") else { break };
        let target = &after[open + 2..];
        let Some(close) = target.find(')') else { break };
        let path = target[..close]
            .trim()
            .trim_matches(|c| c == '<' || c == '>' || c == '"');
        let remote = path.contains("://") || path.starts_with("data:");
        if !path.is_empty() && !remote {
            let full = if std::path::Path::new(path).is_absolute() {
                std::path::PathBuf::from(path)
            } else {
                std::env::current_dir()
                    .map(|dir| dir.join(path))
                    .unwrap_or_default()
            };
            if full.is_file() {
                paths.push(path.to_string());
            }
        }
        rest = &target[close + 1..];
    }
    paths
}

pub(in crate::tui::sigil_view) fn itemized(line: &str) -> bool {
    let text = line.trim_start();
    text.starts_with("[x] ")
        || text.starts_with("[/] ")
        || text.starts_with("[ ] ")
        || text.starts_with("[!] ")
        || text.chars().next().is_some_and(|c| c.is_ascii_digit()) && text.contains(". ")
}

pub(in crate::tui::sigil_view) fn preview(bytes: &[u8]) -> Result<ImageSource, String> {
    decode(bytes, 480, 320)
}

pub(in crate::tui::sigil_view) fn decode(
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<ImageSource, String> {
    let image = load_from_memory(bytes).map_err(|error| format!("invalid image: {error}"))?;
    let rgba = image
        .thumbnail(image.width().min(width), image.height().min(height))
        .into_rgba8();
    ImageSource::from_rgba8(rgba.width(), rgba.height(), rgba.into_raw())
        .ok_or_else(|| "image has no pixels".to_string())
}

pub(in crate::tui::sigil_view) fn tool_markdown(
    name: &str,
    arg: &str,
    state: &ToolState,
    output: &str,
) -> String {
    let mark = match state {
        ToolState::Running => "⏳",
        ToolState::Ok => "✓",
        ToolState::Failed => "✗",
    };
    if output.is_empty() {
        format!("{mark} **{name}** `{arg}`")
    } else {
        format!("{mark} **{name}** `{arg}`\n```text\n{output}\n```")
    }
}
