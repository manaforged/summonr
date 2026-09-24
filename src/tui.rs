use std::path::PathBuf;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::error::Result;
use crate::prefs::Prefs;
use crate::provider::Client;
use crate::provider::UserInput;
use crate::runtime::Loop;
use crate::session::Prompt;
use crate::tools::Event;

mod help;
mod matching;
mod sigil_view;
pub(crate) use help::help_text;
pub(crate) use matching::matching_commands;
pub(crate) use sigil_view::ellipsize;
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
    Peers(Vec<crate::bus::Agent>),
    Files(Vec<String>),
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
    Resume(String),
    LoginStep {
        name: String,
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
    Clipboard,
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

#[derive(Clone, Debug)]
pub struct Info {
    pub version: String,
    pub model: String,
    pub relay: Option<String>,
    pub session: String,
    pub endpoint: String,
    pub pick: Option<&'static str>,
    pub prefs: Prefs,
    pub splash: bool,
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

pub(crate) use crate::session::short as short_session;

#[cfg_attr(
    not(feature = "gui"),
    expect(
        dead_code,
        reason = "the desktop panes are the only caller, and they are a gui feature"
    )
)]
fn context(round: Option<&Round>, window: Option<u32>, compact: bool) -> String {
    let Some(window) = window.filter(|window| *window > 0) else {
        return "—".to_string();
    };
    let used = round.map_or(0, Round::total).min(window);
    let percent = u64::from(used) * 100 / u64::from(window);
    if compact {
        return format!("{percent}%");
    }
    format!(
        "{} / {} · {}% used",
        crate::provider::compact(u64::from(used)),
        crate::provider::compact(u64::from(window)),
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
    let picked = after_cd(picked)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
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
