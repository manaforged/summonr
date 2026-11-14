/// What a retryable turn failure ends with; a window offers Retry in its place.
pub const RETRY_HINT: &str = " · F5 or /retry continues";

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::prefs::Prefs;
use crate::provider::UserInput;
use crate::session::Prompt;
use crate::tools::Event;

use super::Activity;

pub enum Msg {
    Fact(Fact),
    Intent(Intent),
}

/// Something the world or the worker reports to the interface.
pub enum Fact {
    Secret(crate::askpass::Request),
    Calling(String),
    Event(Event),
    Busy(Option<Activity>),
    Failed(String),
    Restored(Vec<UserInput>),
    Queued {
        steering: usize,
        queued: usize,
    },
    Pending(Arc<[(bool, String)]>),
    Outcome(String),
    Session(String),
    Settings(Arc<Snapshot>),
    Models {
        request: u64,
        rows: Vec<crate::recent::ModelId>,
        facts: Arc<BTreeMap<crate::recent::ModelId, crate::catalog::Model>>,
        groups: Arc<BTreeMap<String, crate::endpoint::State>>,
        causes: Arc<BTreeMap<String, String>>,
        pinned: usize,
        recent: usize,
        loading: bool,
        error: String,
    },
    ModelNote(String),
    Defaulted(Option<crate::recent::ModelId>),
    Sessions(std::result::Result<Vec<(crate::session::SessionInfo, bool)>, String>),
    Usage(std::result::Result<Vec<crate::session::Spend>, String>),
    Plans(Vec<crate::quota::Reading>),
    Prompts {
        session: Option<String>,
        result: std::result::Result<(Vec<Prompt>, Vec<String>), String>,
    },
    Workflow {
        name: Option<String>,
        names: Vec<String>,
        selections: Vec<Option<String>>,
    },
    Peers(Vec<crate::mail::Agent>),
    Files(Vec<String>),
    Jobs(Vec<(crate::jobs::Record, crate::jobs::Status)>),
    Mcp(Vec<crate::mcp::ServerStatus>),
    Offers(Vec<crate::mcp::Offer>),
    Clipboard(std::result::Result<Vec<u8>, String>),
    Diff(crate::diff::Diff, Option<PathBuf>),
    DiffFailed(String, Option<PathBuf>),
    Trees(Vec<crate::worktree::Status>),
    Panels(Vec<crate::panel::Panel>),
    Panel {
        name: String,
        result: Result<crate::panel::Content, String>,
    },
    Moved(PathBuf),
    Changed(Arc<[String]>),
    Edited(Vec<String>),
    View {
        root: Option<PathBuf>,
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
    Live(Vec<super::Live>),
    /// The current branch's pull request after a forge ask, or why there is none.
    Forge(Result<Option<crate::forge::Pull>, crate::forge::Error>),
    Named(String),
    Approval(String),
    Replayed,
    /// Another window holds this session, so it did not open.
    Held(String),
    /// Another window took this session, which has stopped here.
    TakenOver,
    /// A saved conversation in one message, so a view never draws part of it.
    Transcript {
        /// Whether the view drops what it shows before it draws these events.
        replace: bool,
        /// The conversation's events in journal order.
        events: Vec<Event>,
    },
    LoginStep {
        text: String,
        url: String,
        code: String,
        asking: bool,
    },
    LoginDone {
        name: String,
        result: std::result::Result<String, String>,
        rejected: bool,
    },
}

/// Something the user asks the model to do.
pub enum Intent {
    Reset,
    RunMode(crate::WorkKind),
    OpenLogin(String),
    OpenHelp,
    Resume(String),
    Bye,
}

pub enum Cmd {
    /// Asks the window that holds this session to let go, then opens it here.
    TakeOver(String),
    /// Adds a window pane that shows its own agent.
    Attach(super::View),
    /// Closes the window pane that sends it; its agent keeps running.
    Detach,
    Command(String),
    Login(String),
    SetKey(String, String),
    LoginAnswer(String),
    Logout(String),
    Submit(UserInput),
    Mail,
    Steer(UserInput),
    Answer(u64, String),
    Queue(UserInput),
    Send(Vec<String>, String),
    SetEffort(Option<crate::provider::Effort>),
    SetVerify(bool),
    ToggleApproval,
    SetModel(String),
    SelectModel(String, String),
    ListModels {
        request: u64,
        only: Option<String>,
        cancel: tokio::sync::oneshot::Receiver<()>,
    },
    Pin(crate::recent::ModelId, bool),
    SaveDefault(Option<crate::recent::ModelId>),
    ListSessions,
    ListUsage,
    ReadPlans {
        due: bool,
    },
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
    SessionDiff(String),
    Patch(Option<std::path::PathBuf>, String),
    Review {
        root: Option<std::path::PathBuf>,
        path: String,
        hunk: String,
        op: crate::hunks::Op,
    },
    Trees,
    ListPanels,
    RunPanel(String),
    /// Starts a stream panel's command while the panel shows.
    OpenPanel(String),
    /// Stops a stream panel's command.
    ClosePanel(String),
    /// Runs a panel's command again with a pressed A2UI button's event on stdin.
    ActPanel {
        /// The panel's name.
        name: String,
        /// The event's name.
        event: String,
        /// The event's context, its bound paths resolved.
        context: serde_json::Value,
    },
    /// Asks the workspace's GitHub repository through `gh`.
    Forge(crate::forge::Ask),
    View(Option<std::path::PathBuf>, String),
    Save(PathBuf, String),
    Merge(std::path::PathBuf),
    Discard(std::path::PathBuf),
    AssignModel(crate::workflow::Role, String, String),
    ClearRole(crate::workflow::Role),
    SaveWorkflow(String),
    ListWorkflows,
    SetMail(bool),
    SetModule(crate::modules::ModuleId, bool),
    SetChrome(std::path::PathBuf),
    Dequeue,
    SteerQueued,
    DeleteSession(String),
    Stop(String),
    MarkUnread(String),
    Rename {
        session: String,
        name: String,
    },
    Export(Option<String>),
    Cancel,
    CancelLogin,
    Open(super::Open),
    Replay,
    Quit,
    Tick,
    Peers(std::sync::Arc<[crate::mail::Agent]>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Round {
    pub prompt: u32,
    pub completion: u32,
    pub reasoning: u32,
    pub cached: u32,
    pub prefix_hash: String,
    pub cost: Option<f64>,
    pub side: bool,
}

impl Round {
    pub fn total(&self) -> u32 {
        self.prompt.saturating_add(self.completion)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub model: String,
    /// The folder this session works in.
    pub workspace: std::path::PathBuf,
    pub relay: Option<String>,
    pub mode: crate::WorkKind,
    pub effort: Option<crate::provider::Effort>,
    pub levels: Vec<crate::provider::Effort>,
    pub verify: Option<String>,
    pub strict: bool,
    pub approval: bool,
    pub endpoint: String,
    pub context_window: Option<u32>,
    pub workflow: Option<String>,
    pub unconfigured: bool,
    pub prompt_system: usize,
    pub prompt_schemas: usize,
    pub prompt_tools: usize,
    pub prompt_other: usize,
}

impl Snapshot {
    #[must_use]
    pub fn model_label(&self, suffix: &str) -> String {
        if self.endpoint.is_empty() {
            format!("{}{suffix}", self.model)
        } else {
            format!("{}{suffix} · {}", self.model, self.endpoint)
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
    pub pick: Option<crate::endpoint::Origin>,
    pub prefs: Prefs,
    pub splash: bool,
    pub inline: bool,
    pub strict: bool,
    pub workspace: String,
    pub workflow: Option<crate::workflow::Workflow>,
    pub config: Option<std::path::PathBuf>,
    pub prompt: Option<String>,
    pub trust: bool,
}
