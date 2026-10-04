use std::path::PathBuf;

use crate::error::Result;
use crate::prefs::Prefs;
use crate::provider::Client;
use crate::provider::UserInput;
use crate::runtime::Loop;
use crate::session::Prompt;
use crate::tools::Event;

mod channel;
mod help;
pub mod hub;
mod matching;
mod sigil_view;
pub use channel::{Inbox, Ui, channel};
pub(crate) use help::help_text;
pub(crate) use matching::matching_commands;
pub use sigil_view::profile::run as profile;
mod worker;

use worker::worker;

pub(crate) enum Msg {
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
    OpenLogin(String),
    OpenHelp,
    Settings(Snapshot),
    Models {
        request: u64,
        rows: Vec<crate::recent::Seat>,
        facts: std::collections::BTreeMap<crate::recent::Seat, crate::catalog::Model>,
        groups: std::collections::BTreeMap<String, crate::endpoint::State>,
        causes: std::collections::BTreeMap<String, String>,
        pinned: usize,
        recent: usize,
        loading: bool,
        error: String,
    },
    ModelNote(String),
    Defaulted(Option<crate::recent::Seat>),
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
    Peers(Vec<crate::mail::Agent>),
    Files(Vec<String>),
    Jobs(Vec<(crate::jobs::Record, crate::jobs::Status)>),
    Mcp(Vec<crate::mcp::ServerStatus>),
    Offers(Vec<crate::mcp::Offer>),
    Clipboard(std::result::Result<Vec<u8>, String>),
    Diff(crate::diff::Diff),
    DiffFailed(String),
    Trees(Vec<crate::worktree::Status>),
    Changed(Vec<String>),
    Edited(Vec<String>),
    View {
        path: String,
        file: PathBuf,
        text: String,
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
    Review {
        root: Option<PathBuf>,
        result: std::result::Result<crate::hunks::Marks, String>,
    },
    Resume(String),
    Live(Vec<hub::Live>),
    Replayed,
    LoginStep {
        text: String,
        url: String,
        code: String,
        asking: bool,
    },
    LoginDone {
        name: String,
        result: std::result::Result<String, String>,
    },
    Bye,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Activity {
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

enum Cmd {
    Command(String),
    Login(String),
    SetKey(String, String),
    LoginAnswer(String),
    Logout(String),
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
        only: Option<String>,
        cancel: tokio::sync::oneshot::Receiver<()>,
    },
    Pin(crate::recent::Seat, bool),
    SaveDefault(Option<crate::recent::Seat>),
    ListSessions,
    ListUsage,
    ListPrompts(Option<String>),
    Branch {
        session: String,
        turn: usize,
    },
    ListFiles,
    Jobs,
    Mcp,
    Offers,
    KillJob(String),
    Clipboard,
    Diff(Option<std::path::PathBuf>),
    Patch(Option<std::path::PathBuf>, String),
    Review {
        root: Option<std::path::PathBuf>,
        path: String,
        hunk: String,
        op: crate::hunks::Op,
    },
    Trees,
    View(Option<std::path::PathBuf>, String),
    Save(PathBuf, String),
    Merge(std::path::PathBuf),
    Discard(std::path::PathBuf),
    SeatRole(crate::workflow::Role, String, String),
    ClearRole(crate::workflow::Role),
    SaveWorkflow(String),
    ListWorkflows,
    SetMail(bool),
    SetModule(crate::modules::ModuleId, bool),
    Dequeue,
    SteerQueued,
    DeleteSession(String),
    Export(Option<String>),
    Cancel,
    Open(hub::Open),
    Replay,
    Quit,
    Tick,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Round {
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
pub(crate) struct Snapshot {
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

impl Snapshot {
    #[must_use]
    pub(crate) fn mode_name(&self) -> &'static str {
        if self.plan {
            crate::RunMode::Plan.name()
        } else if self.work {
            crate::RunMode::Work.name()
        } else if self.goal {
            crate::RunMode::Goal.name()
        } else {
            crate::RunMode::Auto.name()
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
    pub pick: Option<crate::endpoint::Pick>,
    pub prefs: Prefs,
    pub splash: bool,
    pub inline: bool,
    pub plan: bool,
    pub strict: bool,
    pub workspace: String,
    pub workflow: Option<crate::workflow::Workflow>,
    pub config: Option<std::path::PathBuf>,
}

pub fn run(
    loop_: Loop<Client>,
    workspace: PathBuf,
    info: Info,
    ui: Ui,
    inbox: Inbox,
    gui: bool,
    factory: hub::Factory,
) -> Result<i32> {
    let (ui_tx, ui_rx) = (ui.0, inbox.0);
    if gui {
        #[cfg(feature = "gui")]
        return sigil_view::run_gui(loop_, workspace, info, ui_tx, ui_rx, factory);
        #[cfg(not(feature = "gui"))]
        return Err(crate::error::Error::Config(
            "this build has no window; install with `cargo install --features gui`".to_string(),
        ));
    }
    sigil_view::run(loop_, workspace, info, ui_tx, ui_rx, factory)
}

#[derive(Clone, Debug)]
enum ToolState {
    Running,
    Ok,
    Failed,
}

const QUIET_OPS: [&str; 2] = ["file", "sequence"];

pub(crate) fn tool_arg(arguments: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return String::new();
    };
    let field = |name: &str| value.get(name).and_then(serde_json::Value::as_str);
    if let Some(pattern) = field("pattern") {
        let scope = field("glob").or_else(|| field("path")).unwrap_or("");
        return format!("search \"{pattern}\" {scope}")
            .trim_end()
            .to_string();
    }
    let paths = value
        .get("paths")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        });
    let picked = ["path", "command", "name", "query", "url"]
        .iter()
        .find_map(|name| field(name))
        .map(str::to_string)
        .or(paths)
        .unwrap_or_default();
    let mut picked = after_cd(&picked)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for key in ["line", "character"] {
        if let Some(number) = value.get(key).and_then(serde_json::Value::as_u64) {
            picked.push_str(&format!(":{number}"));
        }
    }
    if let Some(op) = field("op").filter(|op| !QUIET_OPS.contains(op)) {
        picked.push_str(&format!(" {op}"));
    }
    if value.get("references").and_then(serde_json::Value::as_bool) == Some(true) {
        picked.push_str(" refs");
    }
    picked
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
