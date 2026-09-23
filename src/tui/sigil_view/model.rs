use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::time::Instant;

use image::load_from_memory;
use sigil::prelude::ImageSource;

use crate::prefs::{Prefs, ToolOutput};
use crate::tools::Event;

use crate::tui::{Activity, Info, Msg, Round, Snapshot, ToolState, tool_arg};

pub(super) use super::app;
use super::speed::Speed;

mod entry;
mod event;

pub(in crate::tui::sigil_view) use entry::{Entry, decode, entry_markdown, itemized, preview};
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
    pub(super) entries: Entries,
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
    pub(super) trees: Vec<crate::worktree::Status>,
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
    pub(super) spinner: usize,
    pub(super) speed: Speed,
    pub(super) queued: (usize, usize),
    pub(super) busy_since: Option<(Activity, Instant)>,
    pub(super) live_tool: Option<(String, String)>,
    pub(super) agents: Vec<Agent>,
    pub(super) agents_hidden: bool,
    pub(super) prefs: Prefs,
    pub(super) snapshot: Snapshot,
    pub(super) peers: Vec<crate::bus::Agent>,
}

impl Model {
    pub(super) fn is_model(&self, choice: &str) -> bool {
        match choice.split_once('\t') {
            Some((endpoint, name)) => {
                endpoint == self.snapshot.endpoint && name == self.snapshot.model
            }
            None => choice == self.snapshot.model,
        }
    }

    pub(super) fn new(info: &Info) -> Self {
        Self {
            session: info.session.clone(),
            entries: Entries::default(),
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
            trees: Vec::new(),
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
            spinner: 0,
            speed: Speed::default(),
            queued: (0, 0),
            busy_since: None,
            live_tool: None,
            agents: Vec::new(),
            agents_hidden: false,
            prefs: info.prefs.clone(),
            snapshot: Snapshot {
                model: info.model.clone(),
                relay: info.relay.clone(),
                plan: info.plan,
                strict: info.strict,
                endpoint: info.endpoint.clone(),
                ..Snapshot::default()
            },
            peers: Vec::new(),
        }
    }

    pub(super) fn apply(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Session(id) => self.session = id,
            Msg::User(text) => self.entries.push(Entry::User(text)),
            Msg::Image(image) => self.entries.push(
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
                },
            ),
            Msg::Receipt(text) => self.receipt(&text),
            Msg::Bye => return false,
            Msg::Calling(name) => self.live_tool = Some((name, "…".to_string())),
            Msg::Event(event) => self.apply_event(event),
            Msg::Busy(busy) => self.set_busy(busy),
            Msg::Failed(text) => {
                self.set_busy(None);
                self.entries.push(Entry::Error(text));
            }
            Msg::Restored(_) => self.queued = (0, 0),
            Msg::Queued { steering, queued } => self.queued = (steering, queued),
            Msg::System(text) => self.entries.push(Entry::System(text)),
            Msg::Cleared => self.clear(),
            Msg::Reset => {
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
                self.speed = Speed::default();
            }
            Msg::RunMode(mode) => {
                self.snapshot.plan = mode == crate::RunMode::Plan;
                self.snapshot.work = mode == crate::RunMode::Work;
                self.snapshot.goal = mode == crate::RunMode::Goal;
            }
            Msg::OpenSettings | Msg::OpenHelp | Msg::Secret(_) => {}
            Msg::Settings(snapshot) => self.snapshot = snapshot,
            Msg::Models { .. }
            | Msg::Sessions(_)
            | Msg::Usage(_)
            | Msg::Prompts { .. }
            | Msg::Workflow { .. } => {}
            Msg::Mail { from, text } => self.entries.push(Entry::Mail { from, text }),
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
            Msg::Saved { .. } => {}
            Msg::Patch { path, result, .. } => self.patch = Some((path, result)),
            Msg::Resume(_) => {}
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
}
