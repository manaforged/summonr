use std::path::PathBuf;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::command::Category;
use crate::error::Result;
use crate::prefs::Prefs;
use crate::provider::Client;
use crate::provider::UserInput;
use crate::runtime::Loop;
use crate::session::Prompt;
use crate::tools::Event;

mod help;
mod sigil_view;
use help::help_text;
pub use sigil_view::profile::run as profile;
mod worker;

use worker::worker;

pub enum Msg {
    Secret(crate::askpass::Request),
    User(String),
    Image(crate::provider::Image),
    Receipt(String),
    Calling(String),
    Event(Event),
    Busy(Option<Activity>),
    Failed(String),
    Restored(Vec<UserInput>),
    Queued {
        steering: usize,
        queued: usize,
    },
    System(String),
    Session(String),
    Cleared,
    Reset,
    RunMode(crate::RunMode),
    OpenSettings,
    OpenHelp,
    Settings(Snapshot),
    Models {
        request: u64,
        rows: Vec<String>,
        recent: usize,
        loading: bool,
        error: String,
    },
    Sessions(std::result::Result<Vec<(crate::session::SessionInfo, bool)>, String>),
    Usage(std::result::Result<Vec<crate::session::Spend>, String>),
    Prompts {
        session: Option<String>,
        result: std::result::Result<(Vec<Prompt>, Vec<String>), String>,
    },
    Workflow {
        name: Option<String>,
        names: Vec<String>,
        seats: Vec<Option<String>>,
    },
    Mail {
        from: String,
        text: String,
    },
    Peers(Vec<crate::bus::Agent>),
    Files(Vec<String>),
    Diff(crate::diff::Diff),
    DiffFailed(String),
    Trees(Vec<crate::worktree::Status>),
    Changed(Vec<String>),
    Edited(Vec<String>),
    View {
        path: String,
        file: PathBuf,
        rope: sigil_ide_core::buffer::Rope,
        added: Vec<(usize, usize)>,
        image: Option<Vec<u8>>,
    },
    Saved {
        file: PathBuf,
        text: String,
        result: std::result::Result<(), String>,
    },
    Patch {
        root: Option<PathBuf>,
        path: String,
        result: std::result::Result<String, String>,
    },
    Resume(String),
    Bye,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Turn,
    Login,
    Compact,
    Branch,
    Shell,
}

impl Activity {
    const fn label(self) -> &'static str {
        match self {
            Self::Turn => "working",
            Self::Login => "signing in",
            Self::Compact => "compacting",
            Self::Branch => "branching",
            Self::Shell => "running",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Resume(String),
}

enum Cmd {
    Command(String),
    Login(String),
    SetKey(String, String),
    Submit(UserInput),
    Mail,
    Steer(UserInput),
    Queue(UserInput),
    Send(Vec<String>, String),
    SetRunMode(crate::RunMode),
    SetEffort(Option<crate::provider::Effort>),
    SetVerify(bool),
    SetEndpoint(String),
    SetModel(String),
    SetSeat(String, String),
    ListModels {
        request: u64,
        cancel: tokio::sync::oneshot::Receiver<()>,
    },
    ListSessions,
    ListUsage,
    ListPrompts(Option<String>),
    Branch {
        session: String,
        turn: usize,
    },
    ListFiles,
    Diff(Option<std::path::PathBuf>),
    Patch(Option<std::path::PathBuf>, String),
    Trees,
    View(Option<std::path::PathBuf>, String),
    Save(PathBuf, String),
    Merge(std::path::PathBuf),
    Discard(std::path::PathBuf),
    SeatRole(crate::workflow::Role, String, String),
    ClearRole(crate::workflow::Role),
    SaveWorkflow(String),
    ListWorkflows,
    SetBus(bool),
    SetModule(String, bool),
    Dequeue,
    DeleteSession(String),
    Export(Option<String>),
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub model: String,
    pub relay: Option<String>,
    pub plan: bool,
    pub work: bool,
    pub goal: bool,
    pub effort: Option<crate::provider::Effort>,
    pub pending_plan: bool,
    pub verify: Option<String>,
    pub strict: bool,
    pub endpoint: String,
    pub context_window: Option<u32>,
    pub workflow: Option<String>,
}

impl Snapshot {}

#[derive(Clone, Debug)]
pub struct Info {
    pub version: String,
    pub model: String,
    pub relay: Option<String>,
    pub session: String,
    pub endpoint: String,
    pub prefs: Prefs,
    pub splash: bool,
    pub plan: bool,
    pub strict: bool,
    pub workspace: String,
    pub workflow: Option<crate::workflow::Workflow>,
    pub config: Option<std::path::PathBuf>,
}

#[must_use]
pub fn matching_commands(typed: &str) -> Vec<(&'static str, &'static str)> {
    if !typed.starts_with('/') || typed.contains('\n') || typed.contains('\r') {
        return Vec::new();
    }
    let query = typed.to_lowercase();
    let needle = query.trim_start_matches('/');
    let mut prefix: Vec<_> = crate::command_specs()
        .iter()
        .filter(|spec| {
            if needle.is_empty() {
                matches!(spec.category, Category::Common | Category::Contextual)
            } else {
                spec.name.trim_start_matches('/').starts_with(needle)
            }
        })
        .collect();
    if !prefix.is_empty() {
        prefix.sort_by_key(|spec| spec.category == Category::Shortcuts);
        return prefix
            .into_iter()
            .map(|spec| (spec.name, spec.description))
            .collect();
    }
    if needle.contains(char::is_whitespace) {
        return Vec::new();
    }
    let mut matches: Vec<_> = crate::command_specs()
        .iter()
        .enumerate()
        .filter_map(|(index, spec)| {
            let name = spec.name.trim_start_matches('/');
            rank(name, spec.description, needle).map(|score| {
                (
                    score,
                    spec.category == Category::Shortcuts,
                    index,
                    spec.name,
                    spec.description,
                )
            })
        })
        .collect();
    matches.sort_unstable_by_key(|(score, shortcut, index, _, _)| (*score, *shortcut, *index));
    if let Some(best) = matches.first().map(|row| row.0) {
        matches.retain(|(score, _, _, _, _)| *score == best);
    }
    matches
        .into_iter()
        .map(|(_, _, _, name, description)| (name, description))
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
) -> Result<Exit> {
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
    if tokens >= 1_000_000 {
        format!(
            "{:.1}m",
            f64::from(u32::try_from(tokens).unwrap_or(u32::MAX)) / 1_000_000.0
        )
    } else if tokens >= 1000 {
        format!(
            "{:.1}k",
            f64::from(u32::try_from(tokens).unwrap_or(u32::MAX)) / 1000.0
        )
    } else {
        tokens.to_string()
    }
}

#[cfg_attr(
    not(feature = "gui"),
    expect(
        dead_code,
        reason = "the desktop panes are the only caller, and they are a gui feature"
    )
)]
fn context(round: Option<&Round>, window: Option<u32>, compact: bool) -> String {
    let Some(window) = window.filter(|window| *window > 0) else {
        return "not set".to_string();
    };
    let used = round.map_or(0, Round::total).min(window);
    let percent = u64::from(used) * 100 / u64::from(window);
    if compact {
        return format!("{percent}%");
    }
    format!(
        "{} / {} · {}% used",
        fmt_tokens(u64::from(used)),
        fmt_tokens(u64::from(window)),
        percent,
    )
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
    let picked = after_cd(picked);
    let chars: Vec<char> = picked.chars().collect();
    if chars.len() > 46 {
        format!("{}…", chars[..45].iter().collect::<String>())
    } else {
        picked.to_string()
    }
}

fn after_cd(command: &str) -> &str {
    let mut rest = command;
    while let Some(target) = rest.strip_prefix("cd ") {
        let cut = [
            target.find("&&").map(|index| (index, 2)),
            target.find(';').map(|index| (index, 1)),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|(index, _)| *index);
        let Some((index, len)) = cut else { break };
        rest = target[index + len..].trim_start();
    }
    rest
}
