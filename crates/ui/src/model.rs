use std::collections::{BTreeSet, HashMap};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::time::Instant;

use image::load_from_memory;
use sigil::prelude::ImageSource;

use summonr_agent::prefs::{Prefs, ToolOutput};
use summonr_agent::tools::Event;

use crate::{ToolState, tool_arg};
use summonr_agent::hub::{Activity, Fact, Info, Intent, Msg, Round, Snapshot};

pub(super) use super::app;
use super::panes::ledger::LedgerRow;
use super::speed::Speed;

mod children;
mod entry;
mod event;

pub(crate) use entry::{Call, Entry, decode, entry_markdown, preview};
pub(crate) use state::{Agent, Doing, Entries, View};
mod state;
mod tools;

impl Deref for Entries {
    type Target = [Entry];

    fn deref(&self) -> &[Entry] {
        &self.inner
    }
}

impl DerefMut for Entries {
    fn deref_mut(&mut self) -> &mut [Entry] {
        &mut self.inner
    }
}

impl<'a> IntoIterator for &'a Entries {
    type Item = &'a Entry;
    type IntoIter = std::slice::Iter<'a, Entry>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl Extend<Entry> for Entries {
    fn extend<I: IntoIterator<Item = Entry>>(&mut self, iter: I) {
        self.inner.extend(iter);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum StatusView {
    #[default]
    Closed,
    Tasks,
    Agents,
}

impl StatusView {
    pub(crate) fn open(self) -> bool {
        self != Self::Closed
    }
}

#[derive(Clone)]
pub(super) struct Model {
    pub(super) session: String,
    pub(super) live: Vec<summonr_agent::hub::Live>,
    pub(super) entries: Entries,
    committed: usize,
    approval_at: Option<(usize, String)>,
    pub(super) ledger: Vec<LedgerRow>,
    pub(super) child_todos: Vec<(String, Vec<LedgerRow>)>,
    pub(super) plan_current: Option<usize>,
    pub(super) plan_total: usize,
    pub(super) plan_objective: String,
    pub(super) files: Vec<String>,
    pub(super) diff: summonr_agent::diff::Diff,
    pub(super) diff_error: Option<String>,
    pub(super) patch: Option<(String, Result<String, String>)>,
    pub(super) marks: Option<(Option<PathBuf>, summonr_agent::hunks::Marks)>,
    pub(super) trees: Vec<summonr_agent::worktree::Status>,
    pub(super) jobs: Vec<(summonr_agent::jobs::Record, summonr_agent::jobs::Status)>,
    pub(super) mcp: Vec<summonr_agent::mcp::ServerStatus>,
    pub(super) offers: Vec<summonr_agent::mcp::Offer>,
    pub(super) view: Option<View>,
    pub(super) recent: Vec<String>,
    tools: HashMap<String, usize>,
    pub(super) usage: Vec<Round>,
    pub(super) unreported_tokens: u64,
    pub(super) rounds: usize,
    pub(super) speed: Speed,
    pub(super) queued: (usize, usize),
    pub(super) pending: Vec<(bool, String)>,
    pub(super) busy_since: Option<(Activity, Instant)>,
    pub(super) doing: Option<Doing>,
    pub(super) agents: Vec<Agent>,
    pub(super) agents_hidden: BTreeSet<String>,
    pub(super) status_view: StatusView,
    pub(super) prefs: Prefs,
    pub(super) snapshot: Snapshot,
    pub(super) pick: Option<summonr_agent::endpoint::Origin>,
    pub(super) peers: Vec<summonr_agent::mail::Agent>,
}

impl Model {
    pub(crate) fn spend(&self) -> Option<f64> {
        self.usage
            .iter()
            .any(|round| round.cost.is_some())
            .then(|| self.usage.iter().filter_map(|round| round.cost).sum())
    }

    pub(crate) fn tokens(&self, field: fn(&Round) -> u32) -> u64 {
        self.usage.iter().map(|round| u64::from(field(round))).sum()
    }

    pub(crate) fn commit_lines(&mut self) -> String {
        let len = self.entries.len();
        let last = len.saturating_sub(1);
        let mut out = String::new();
        while self.committed < len {
            let index = self.committed;
            let stable = self.busy_since.is_none()
                || match &self.entries[index] {
                    Entry::Thinking(_) => index < last,
                    Entry::Assistant { done, .. } => *done && index < last,
                    Entry::Tool { calls, .. } => {
                        index < last
                            && calls
                                .iter()
                                .all(|call| !matches!(call.state, ToolState::Running))
                    }
                    Entry::User(_)
                    | Entry::Receipt(_)
                    | Entry::Verify { .. }
                    | Entry::Mail { .. }
                    | Entry::Aborted(_)
                    | Entry::System(_)
                    | Entry::Injected(_)
                    | Entry::Error(_)
                    | Entry::Chart(_)
                    | Entry::Image { .. } => true,
                };
            if !stable {
                break;
            }
            out.push_str(&entry_markdown(&self.entries[index]));
            self.committed += 1;
        }
        out
    }

    pub(super) fn new(info: &Info) -> Self {
        Self {
            session: info.session.clone(),
            live: Vec::new(),
            entries: Entries::default(),
            committed: 0,
            approval_at: None,
            ledger: Vec::new(),
            child_todos: Vec::new(),
            plan_current: None,
            plan_total: 0,
            plan_objective: String::new(),
            files: Vec::new(),
            diff: summonr_agent::diff::Diff::default(),
            diff_error: None,
            patch: None,
            marks: None,
            trees: Vec::new(),
            jobs: Vec::new(),
            mcp: Vec::new(),
            offers: Vec::new(),
            view: None,
            recent: Vec::new(),
            tools: HashMap::new(),
            usage: Vec::new(),
            unreported_tokens: 0,
            rounds: 0,
            speed: Speed::default(),
            queued: (0, 0),
            pending: Vec::new(),
            busy_since: None,
            doing: None,
            agents: Vec::new(),
            agents_hidden: BTreeSet::new(),
            status_view: StatusView::Closed,
            prefs: info.prefs.clone(),
            snapshot: Snapshot {
                model: info.model.clone(),
                relay: info.relay.clone(),
                strict: info.strict,
                endpoint: info.endpoint.clone(),
                ..Snapshot::default()
            },
            peers: Vec::new(),
            pick: info.pick,
        }
    }

    pub(super) fn apply(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Intent(Intent::Bye) => return false,
            Msg::Fact(Fact::Live(live)) => self.live = live,
            Msg::Fact(Fact::Event(event)) => self.apply_event(event),
            msg @ (Msg::Intent(Intent::User(_))
            | Msg::Intent(Intent::Image(_))
            | Msg::Fact(Fact::Receipt(_))
            | Msg::Fact(Fact::System(_))
            | Msg::Fact(Fact::Injected(_))
            | Msg::Fact(Fact::Approval(_))
            | Msg::Fact(Fact::Outcome(_))
            | Msg::Fact(Fact::Mail { .. })
            | Msg::Fact(Fact::Failed(_))) => self.entry(msg),
            msg @ (Msg::Fact(Fact::Session(_))
            | Msg::Fact(Fact::Calling(_))
            | Msg::Fact(Fact::Busy(_))
            | Msg::Fact(Fact::Queued { .. })
            | Msg::Fact(Fact::Pending(_))
            | Msg::Fact(Fact::Cleared)
            | Msg::Intent(Intent::Reset)
            | Msg::Intent(Intent::RunMode(_))
            | Msg::Fact(Fact::Settings(_))) => self.status(msg),
            msg @ (Msg::Fact(Fact::Peers(_))
            | Msg::Fact(Fact::Files(_))
            | Msg::Fact(Fact::Diff(..))
            | Msg::Fact(Fact::DiffFailed(..))
            | Msg::Fact(Fact::Trees(_))
            | Msg::Fact(Fact::Offers(_))
            | Msg::Fact(Fact::Jobs(_))
            | Msg::Fact(Fact::Mcp(_))
            | Msg::Fact(Fact::Changed(_))
            | Msg::Fact(Fact::Edited(_))
            | Msg::Fact(Fact::View { .. })
            | Msg::Fact(Fact::Patch { .. })
            | Msg::Fact(Fact::Review { .. })) => self.pane_data(msg),
            Msg::Intent(Intent::OpenLogin(_))
            | Msg::Intent(Intent::OpenHelp)
            | Msg::Fact(Fact::Secret(_))
            | Msg::Fact(Fact::Clipboard(_))
            | Msg::Fact(Fact::Moved(_))
            | Msg::Fact(Fact::Named(_))
            | Msg::Fact(Fact::Models { .. })
            | Msg::Fact(Fact::ModelNote(_))
            | Msg::Fact(Fact::Defaulted(_))
            | Msg::Fact(Fact::Sessions(_))
            | Msg::Fact(Fact::Usage(_))
            | Msg::Fact(Fact::Panels(_))
            | Msg::Fact(Fact::Panel { .. })
            | Msg::Fact(Fact::Forge(_))
            | Msg::Fact(Fact::Prompts { .. })
            | Msg::Fact(Fact::Workflow { .. })
            | Msg::Intent(Intent::Resume(_))
            | Msg::Fact(Fact::Replayed)
            | Msg::Fact(Fact::Transcript(_))
            | Msg::Fact(Fact::Held(_))
            | Msg::Fact(Fact::TakenOver)
            | Msg::Fact(Fact::LoginStep { .. })
            | Msg::Fact(Fact::LoginDone { .. })
            | Msg::Fact(Fact::Restored(_))
            | Msg::Fact(Fact::Saved { .. }) => {}
        }
        true
    }

    fn entry(&mut self, msg: Msg) {
        match msg {
            Msg::Intent(Intent::User(text)) => self.entries.push(Entry::User(text)),
            Msg::Intent(Intent::Image(image)) => self.entries.push(image_entry(image)),
            Msg::Fact(Fact::Receipt(text)) => self.receipt(&text),
            Msg::Fact(Fact::System(text)) => self.entries.push(Entry::System(text)),
            Msg::Fact(Fact::Injected(text)) => self.entries.push(Entry::Injected(text)),
            Msg::Fact(Fact::Approval(text)) => self.approval_note(text),
            Msg::Fact(Fact::Outcome(text)) => self.trail(Entry::System(text)),
            Msg::Fact(Fact::Mail { from, text }) => self.entries.push(Entry::Mail { from, text }),
            Msg::Fact(Fact::Failed(text)) => {
                self.set_busy(None);
                self.entries.push(Entry::Error(text));
            }
            _msg => unreachable!("apply routes only entry messages"),
        }
    }

    fn status(&mut self, msg: Msg) {
        match msg {
            Msg::Fact(Fact::Session(id)) => self.session = id,
            Msg::Fact(Fact::Calling(name)) => {
                self.doing = Some(Doing::Tool {
                    name,
                    arg: "…".to_string(),
                })
            }
            Msg::Fact(Fact::Busy(busy)) => self.set_busy(busy),
            Msg::Fact(Fact::Queued { steering, queued }) => self.queued = (steering, queued),
            Msg::Fact(Fact::Pending(rows)) => self.pending = rows,
            Msg::Fact(Fact::Cleared) => self.clear(),
            Msg::Intent(Intent::Reset) => self.reset(),
            Msg::Intent(Intent::RunMode(mode)) => {
                self.snapshot.work = mode == summonr_agent::WorkKind::Steps;
                self.snapshot.goal = mode == summonr_agent::WorkKind::Goal;
            }
            Msg::Fact(Fact::Settings(snapshot)) => {
                if (&snapshot.endpoint, &snapshot.model)
                    != (&self.snapshot.endpoint, &self.snapshot.model)
                {
                    self.pick = None;
                }
                self.snapshot = snapshot;
            }
            _msg => unreachable!("apply routes only status messages"),
        }
    }

    fn pane_data(&mut self, msg: Msg) {
        match msg {
            Msg::Fact(Fact::Peers(peers)) => self.peers = peers,
            Msg::Fact(Fact::Files(files)) => self.files = files,
            Msg::Fact(Fact::Diff(diff, _)) => {
                self.diff = diff;
                self.diff_error = None;
            }
            Msg::Fact(Fact::DiffFailed(error, _)) => {
                self.patch = None;
                self.diff_error = Some(error);
            }
            Msg::Fact(Fact::Trees(trees)) => self.trees = trees,
            Msg::Fact(Fact::Offers(offers)) => self.offers = offers,
            Msg::Fact(Fact::Jobs(jobs)) => self.jobs = jobs,
            Msg::Fact(Fact::Mcp(servers)) => self.mcp = servers,
            Msg::Fact(Fact::Changed(_)) => {}
            Msg::Fact(Fact::Edited(paths)) => self.recent = paths,
            Msg::Fact(Fact::View {
                path,
                file,
                text,
                added,
                image,
                ..
            }) => {
                self.view = Some(View {
                    path,
                    file,
                    text,
                    added,
                    image: image.and_then(|bytes| decode(&bytes, 1920, 1200).ok()),
                })
            }
            Msg::Fact(Fact::Patch { path, result, .. }) => self.patch = Some((path, result)),
            Msg::Fact(Fact::Review { root, result }) => self.review_marks(root, result),
            _msg => unreachable!("apply routes only pane data"),
        }
    }

    fn review_marks(
        &mut self,
        root: Option<PathBuf>,
        result: Result<summonr_agent::hunks::Marks, String>,
    ) {
        match result {
            Ok(marks) => self.marks = Some((root, marks)),
            Err(error) => self
                .entries
                .push(Entry::Error(format!("review failed: {error}"))),
        }
    }

    fn reset(&mut self) {
        self.clear_transcript();
        self.status_view = StatusView::Closed;
        self.recent.clear();
        self.diff = summonr_agent::diff::Diff::default();
        self.usage.clear();
        self.unreported_tokens = 0;
        self.rounds = 0;
        self.doing = None;
        self.agents_hidden.clear();
        self.speed = Speed::default();
        self.marks = None;
    }
}

fn image_entry(image: summonr_agent::provider::Image) -> Entry {
    match image
        .bytes()
        .map_err(|error| error.to_string())
        .and_then(|bytes| preview(&bytes).map(|source| (bytes, source)))
    {
        Ok((bytes, source)) => Entry::Image {
            label: format!("Attached image · {}", image.media_type),
            source,
            bytes: bytes.into(),
        },
        Err(error) => Entry::Error(format!("cannot show attached image: {error}")),
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
