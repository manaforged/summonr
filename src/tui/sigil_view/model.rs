use std::collections::{BTreeSet, HashMap};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::time::Instant;

use image::load_from_memory;
use sigil::prelude::ImageSource;

use crate::prefs::{Prefs, ToolOutput};
use crate::tools::Event;

use crate::tui::{Activity, Info, Msg, Round, Snapshot, ToolState, tool_arg};

pub(super) use super::app;
use super::speed::Speed;

mod children;
mod entry;
mod event;

pub(in crate::tui::sigil_view) use entry::{
    Call, Entry, decode, entry_markdown, itemized, preview,
};
pub(in crate::tui::sigil_view) use state::{Agent, Entries, View};
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

#[derive(Clone)]
pub(super) struct Model {
    pub(super) session: String,
    pub(super) live: Vec<crate::tui::hub::Live>,
    pub(super) entries: Entries,
    committed: usize,
    pub(super) ledger: Vec<String>,
    pub(super) todos_expanded: bool,
    pub(super) todos_offset: usize,
    pub(super) plan_current: Option<usize>,
    pub(super) plan_total: usize,
    pub(super) plan_objective: String,
    pub(super) files: Vec<String>,
    pub(super) diff: crate::diff::Diff,
    pub(super) diff_error: Option<String>,
    pub(super) patch: Option<(String, Result<String, String>)>,
    pub(super) marks: Option<(Option<PathBuf>, crate::hunks::Marks)>,
    pub(super) trees: Vec<crate::worktree::Status>,
    pub(super) jobs: Vec<(crate::jobs::Record, crate::jobs::Status)>,
    pub(super) mcp: Vec<crate::mcp::ServerStatus>,
    pub(super) offers: Vec<crate::mcp::Offer>,
    pub(super) view: Option<View>,
    pub(super) recent: Vec<String>,
    tools: HashMap<String, usize>,
    pub(super) usage: Vec<Round>,
    pub(super) prompt_tokens: u64,
    pub(super) completion_tokens: u64,
    pub(super) reasoning_tokens: u64,
    pub(super) cached_tokens: u64,
    pub(super) unreported_tokens: u64,
    pub(super) rounds: usize,
    pub(super) speed: Speed,
    pub(super) queued: (usize, usize),
    pub(super) busy_since: Option<(Activity, Instant)>,
    pub(super) live_tool: Option<(String, String)>,
    pub(super) agents: Vec<Agent>,
    pub(super) agents_hidden: BTreeSet<String>,
    pub(super) agents_expanded: bool,
    pub(super) prefs: Prefs,
    pub(super) snapshot: Snapshot,
    pub(super) pick: Option<crate::endpoint::Pick>,
    pub(super) peers: Vec<crate::mail::Agent>,
}

impl Model {
    pub(in crate::tui::sigil_view) fn spend(&self) -> Option<f64> {
        self.usage
            .iter()
            .any(|round| round.cost.is_some())
            .then(|| self.usage.iter().filter_map(|round| round.cost).sum())
    }

    pub(in crate::tui::sigil_view) fn commit_lines(&mut self) -> String {
        let len = self.entries.len();
        let last = len.saturating_sub(1);
        let mut out = String::new();
        while self.committed < len {
            let index = self.committed;
            let stable = self.busy_since.is_none()
                || match &self.entries[index] {
                    Entry::Thinking(_) => index < last,
                    Entry::Assistant { done, .. } => *done && index < last,
                    Entry::Tool { state, .. } => {
                        index < last && !matches!(state, ToolState::Running)
                    }
                    Entry::ToolGroup { calls, .. } => {
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
            ledger: Vec::new(),
            todos_expanded: false,
            todos_offset: 0,
            plan_current: None,
            plan_total: 0,
            plan_objective: String::new(),
            files: Vec::new(),
            diff: crate::diff::Diff::default(),
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
            prompt_tokens: 0,
            completion_tokens: 0,
            reasoning_tokens: 0,
            cached_tokens: 0,
            unreported_tokens: 0,
            rounds: 0,
            speed: Speed::default(),
            queued: (0, 0),
            busy_since: None,
            live_tool: None,
            agents: Vec::new(),
            agents_hidden: BTreeSet::new(),
            agents_expanded: false,
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
            Msg::Bye => return false,
            Msg::Live(live) => self.live = live,
            Msg::Event(event) => self.apply_event(event),
            msg @ (Msg::User(_)
            | Msg::Image(_)
            | Msg::Receipt(_)
            | Msg::System(_)
            | Msg::Outcome(_)
            | Msg::Mail { .. }
            | Msg::Failed(_)) => self.entry(msg),
            msg @ (Msg::Session(_)
            | Msg::Calling(_)
            | Msg::Busy(_)
            | Msg::Queued { .. }
            | Msg::Cleared
            | Msg::Reset
            | Msg::RunMode(_)
            | Msg::Settings(_)) => self.status(msg),
            msg @ (Msg::Peers(_)
            | Msg::Files(_)
            | Msg::Diff(_)
            | Msg::DiffFailed(_)
            | Msg::Trees(_)
            | Msg::Offers(_)
            | Msg::Jobs(_)
            | Msg::Mcp(_)
            | Msg::Changed(_)
            | Msg::Edited(_)
            | Msg::View { .. }
            | Msg::Patch { .. }
            | Msg::Review { .. }) => self.pane_data(msg),
            Msg::OpenLogin(_)
            | Msg::OpenHelp
            | Msg::Secret(_)
            | Msg::Clipboard(_)
            | Msg::Models { .. }
            | Msg::ModelNote(_)
            | Msg::Defaulted(_)
            | Msg::Sessions(_)
            | Msg::Usage(_)
            | Msg::Prompts { .. }
            | Msg::Workflow { .. }
            | Msg::Resume(_)
            | Msg::Replayed
            | Msg::LoginStep { .. }
            | Msg::LoginDone { .. }
            | Msg::Restored(_)
            | Msg::Saved { .. } => {}
        }
        if self
            .ledger
            .iter()
            .all(|line| super::panes::ledger::item(line).0 == '✓')
        {
            self.todos_expanded = false;
            self.todos_offset = 0;
        }
        true
    }

    fn entry(&mut self, msg: Msg) {
        match msg {
            Msg::User(text) => self.entries.push(Entry::User(text)),
            Msg::Image(image) => self.entries.push(image_entry(image)),
            Msg::Receipt(text) => self.receipt(&text),
            Msg::System(text) => self.entries.push(Entry::System(text)),
            Msg::Outcome(text) => self.trail(Entry::System(text)),
            Msg::Mail { from, text } => self.entries.push(Entry::Mail { from, text }),
            Msg::Failed(text) => {
                self.set_busy(None);
                self.entries.push(Entry::Error(text));
            }
            _msg => unreachable!("apply routes only entry messages"),
        }
    }

    fn status(&mut self, msg: Msg) {
        match msg {
            Msg::Session(id) => self.session = id,
            Msg::Calling(name) => self.live_tool = Some((name, "…".to_string())),
            Msg::Busy(busy) => self.set_busy(busy),
            Msg::Queued { steering, queued } => self.queued = (steering, queued),
            Msg::Cleared => self.clear(),
            Msg::Reset => self.reset(),
            Msg::RunMode(mode) => {
                self.snapshot.work = mode == crate::RunMode::Work;
                self.snapshot.goal = mode == crate::RunMode::Goal;
            }
            Msg::Settings(snapshot) => {
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
            Msg::Peers(peers) => self.peers = peers,
            Msg::Files(files) => self.files = files,
            Msg::Diff(diff) => {
                self.diff = diff;
                self.diff_error = None;
            }
            Msg::DiffFailed(error) => {
                self.diff = crate::diff::Diff::default();
                self.patch = None;
                self.diff_error = Some(error);
            }
            Msg::Trees(trees) => self.trees = trees,
            Msg::Offers(offers) => self.offers = offers,
            Msg::Jobs(jobs) => self.jobs = jobs,
            Msg::Mcp(servers) => self.mcp = servers,
            Msg::Changed(_) => {}
            Msg::Edited(paths) => self.recent = paths,
            Msg::View {
                path,
                file,
                text,
                added,
                image,
            } => {
                self.view = Some(View {
                    path,
                    file,
                    text,
                    added,
                    image: image.and_then(|bytes| decode(&bytes, 1920, 1200).ok()),
                })
            }
            Msg::Patch { path, result, .. } => self.patch = Some((path, result)),
            Msg::Review { root, result } => self.review_marks(root, result),
            _msg => unreachable!("apply routes only pane data"),
        }
    }

    fn review_marks(&mut self, root: Option<PathBuf>, result: Result<crate::hunks::Marks, String>) {
        match result {
            Ok(marks) => self.marks = Some((root, marks)),
            Err(error) => self
                .entries
                .push(Entry::Error(format!("review failed: {error}"))),
        }
    }

    fn reset(&mut self) {
        self.committed = 0;
        self.entries.clear();
        self.tools.clear();
        self.ledger.clear();
        self.todos_expanded = false;
        self.todos_offset = 0;
        self.plan_current = None;
        self.plan_total = 0;
        self.plan_objective.clear();
        self.recent.clear();
        self.usage.clear();
        self.prompt_tokens = 0;
        self.completion_tokens = 0;
        self.reasoning_tokens = 0;
        self.cached_tokens = 0;
        self.unreported_tokens = 0;
        self.rounds = 0;
        self.live_tool = None;
        self.agents.clear();
        self.agents_hidden.clear();
        self.speed = Speed::default();
    }
}

fn image_entry(image: crate::provider::Image) -> Entry {
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
