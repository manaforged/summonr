use std::path::PathBuf;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::error::Result;
use crate::prefs::Prefs;
use crate::provider::Client;
use crate::provider::UserInput;
use crate::runtime::Loop;
use crate::tools::Event;

mod sigil_view;
mod worker;

use worker::worker;

pub enum Msg {
    Delta(String),
    Thinking(String),
    Calling(String),
    Event(Event),
    Busy(bool),
    Failed(String),
    System(String),
    Cleared,
    Mode(bool),
    OpenSettings,
    Settings(Snapshot),
    Model {
        model: String,
        relay: Option<String>,
    },
    Models {
        rows: Vec<String>,
        recent: usize,
    },
    ModelsFailed(String),
    Workflow {
        name: Option<String>,
        names: Vec<String>,
        seats: Vec<Option<String>>,
    },
    Mail {
        from: String,
        text: String,
    },
    Peers(usize),
    Files(Vec<String>),
    Diff(crate::diff::Diff),
    Trees(Vec<crate::worktree::Status>),
    Changed(Vec<String>),
    Edited(Vec<String>),
    View {
        path: String,
        rope: sigil_ide_core::buffer::Rope,
        added: Vec<(usize, usize)>,
    },
    Patch {
        path: String,
        text: String,
    },
    Bye,
}

enum Cmd {
    Command(String),
    Login(String),
    SetKey(String, String),
    Submit(UserInput),
    Mail,
    Steer(UserInput),
    Queue(UserInput),
    SetMode(bool),
    SetRounds(usize),
    SetVerify(bool),
    SetEndpoint(String),
    SetModel(String),
    SetSeat(String, String),
    ListModels,
    ListFiles,
    Diff(Option<std::path::PathBuf>),
    Patch(Option<std::path::PathBuf>, String),
    Trees,
    View(Option<std::path::PathBuf>, String),
    Save(String, String),
    Merge(std::path::PathBuf),
    Discard(std::path::PathBuf),
    SeatRole(crate::workflow::Role, String, String),
    ClearRole(crate::workflow::Role),
    SaveWorkflow(String),
    ListWorkflows,
    SetBus(bool),
    Cancel,
    Quit,
    Tick,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Round {
    pub prompt: u32,
    pub completion: u32,
    pub reasoning: u32,
    pub cached: u32,
    pub prefix_hash: String,
    pub cost: Option<f64>,
}

impl Round {
    fn total(&self) -> u32 {
        self.prompt.saturating_add(self.completion)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub model: String,
    pub relay: Option<String>,
    pub plan: bool,
    pub verify: Option<String>,
    pub max_rounds: usize,
    pub strict: bool,
    pub endpoint: String,
    pub context_window: Option<u32>,
    pub workflow: Option<String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            model: String::new(),
            relay: None,
            plan: false,
            verify: None,
            max_rounds: 64,
            strict: false,
            endpoint: String::new(),
            workflow: None,
            context_window: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Info {
    pub version: String,
    pub model: String,
    pub relay: Option<String>,
    pub session: String,
    pub endpoint: String,
    pub prefs: Prefs,
    pub plan: bool,
    pub strict: bool,
    pub workspace: String,
    pub workflow: Option<String>,
    pub config: Option<std::path::PathBuf>,
}

#[must_use]
pub fn matching_commands(typed: &str) -> Vec<(&'static str, &'static str)> {
    if !typed.starts_with('/') || typed.contains('\n') || typed.contains('\r') {
        return Vec::new();
    }
    let query = typed.to_lowercase();
    let needle = query.trim_start_matches('/');
    let prefix: Vec<_> = crate::command_specs()
        .iter()
        .filter(|spec| needle.is_empty() || spec.name.trim_start_matches('/').starts_with(needle))
        .map(|spec| (spec.name, spec.description))
        .collect();
    if !prefix.is_empty() {
        return prefix;
    }
    if needle.contains(char::is_whitespace) {
        return Vec::new();
    }
    let mut matches: Vec<_> = crate::command_specs()
        .iter()
        .enumerate()
        .filter_map(|(index, spec)| {
            let name = spec.name.trim_start_matches('/');
            rank(name, spec.description, needle)
                .map(|score| (score, index, spec.name, spec.description))
        })
        .collect();
    matches.sort_unstable_by_key(|(score, index, _, _)| (*score, *index));
    if let Some(best) = matches.first().map(|row| row.0) {
        matches.retain(|(score, _, _, _)| *score == best);
    }
    matches
        .into_iter()
        .map(|(_, _, name, description)| (name, description))
        .collect()
}

fn rank(name: &str, description: &str, needle: &str) -> Option<u8> {
    if needle.is_empty() || name.starts_with(needle) {
        Some(0)
    } else if name.contains(needle) {
        Some(1)
    } else if description.to_lowercase().contains(needle) {
        Some(2)
    } else if subsequence(name, needle) {
        Some(3)
    } else {
        None
    }
}

fn subsequence(text: &str, needle: &str) -> bool {
    let mut chars = needle.chars();
    let mut next = chars.next();
    for ch in text.chars() {
        if next == Some(ch) {
            next = chars.next();
        }
    }
    next.is_none()
}

pub fn run(
    loop_: Loop<Client>,
    workspace: PathBuf,
    info: Info,
    ui_tx: UnboundedSender<Msg>,
    ui_rx: UnboundedReceiver<Msg>,
    gui: bool,
) -> Result<i32> {
    if gui {
        #[cfg(feature = "gui")]
        return sigil_view::run_gui(loop_, workspace, info, ui_tx, ui_rx);
        #[cfg(not(feature = "gui"))]
        return Err(crate::error::Error::Config(
            "this build has no window; install with `cargo install --features gui`".to_string(),
        ));
    }
    sigil_view::run(loop_, workspace, info, ui_tx, ui_rx)
}

fn short_session(id: &str) -> String {
    let start = id.len().saturating_sub(5);
    id.get(start..).unwrap_or(id).to_string()
}

fn fmt_tokens(tokens: u64) -> String {
    if tokens >= 1000 {
        format!(
            "{:.1}k",
            f64::from(u32::try_from(tokens).unwrap_or(u32::MAX)) / 1000.0
        )
    } else {
        tokens.to_string()
    }
}

#[derive(Clone, Debug)]
enum ToolState {
    Running,
    Ok,
    Failed,
}

fn tool_arg(arguments: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return String::new();
    };
    let picked = ["path", "command", "pattern", "name", "query", "url"]
        .iter()
        .find_map(|field| value.get(field).and_then(serde_json::Value::as_str))
        .unwrap_or("");
    let chars: Vec<char> = picked.chars().collect();
    if chars.len() > 46 {
        format!("{}…", chars[..45].iter().collect::<String>())
    } else {
        picked.to_string()
    }
}

fn last_line(output: &str) -> String {
    output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(120)
        .collect()
}

fn tail_lines(output: &str, n: usize) -> String {
    let lines: Vec<&str> = output.lines().map(str::trim_end).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

fn help_text() -> String {
    let commands = crate::command_specs()
        .iter()
        .map(|spec| format!("{} {}", spec.name, spec.description))
        .collect::<Vec<_>>()
        .join(" · ");
    format!(
        "{commands}\n\
         @path attach text or an image\n\
         keys: enter send/steer · tab complete/queue · ctrl+j newline · shift+tab mode\n\
         pgup/pgdn or wheel scroll · ctrl+o settings · ctrl+g editor\n\
         panes: ctrl+e files · ctrl+l ledger · ctrl+x diff · /pane worktrees · ctrl+p hide/show · ctrl+→ focus pane · ctrl+← composer\n\
         in a pane: ↑↓ move · ←→ collapse/expand or hunks/list · enter attach @path or view hunks · right-click menu · esc back\n\
         view: → on a file or /view PATH · ↑↓ scroll · o open in $EDITOR · esc back\n\
         diff: t last turn only · c copy the patch\n\
         edit: e in the view pane · type, enter, arrows · ctrl+s save · esc leave (twice to discard)\n\
         ctrl+shift+←/→ resize the split\n\
         settings: enter on model lists every ready seat\n\
         workflow card: ↑↓ role · enter seat · d driving · ←→ saved · s save\n\
         esc cancel · ctrl+c clear/quit · ctrl+d quit when empty"
    )
}

#[cfg(test)]
#[path = "tui_tests.rs"]
mod tests;
