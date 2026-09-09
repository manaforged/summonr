use std::collections::HashMap;
use std::time::Instant;

use image::load_from_memory;
use sigil::prelude::ImageSource;

use crate::prefs::{Prefs, ToolOutput};
use crate::tools::Event;

use super::super::{Activity, Info, Msg, Round, Snapshot, ToolState, tool_arg};

#[derive(Clone, Debug)]
pub(super) enum Entry {
    User(String),
    Thinking(String),
    Assistant {
        text: String,
        done: bool,
    },
    Tool {
        name: String,
        arg: String,
        state: ToolState,
        output: String,
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
        label: String,
    },
}

/// One workspace file in the view pane with its added-line ranges.
#[derive(Clone)]
pub(super) struct View {
    pub(super) path: String,
    pub(super) file: std::path::PathBuf,
    pub(super) rope: sigil_ide_core::buffer::Rope,
    pub(super) added: Vec<(usize, usize)>,
}

#[derive(Clone)]
pub(super) struct Model {
    pub(super) session: String,
    pub(super) entries: Vec<Entry>,
    pub(super) ledger: Vec<String>,
    pub(super) files: Vec<String>,
    pub(super) diff: crate::diff::Diff,
    pub(super) patch: Option<(String, String)>,
    pub(super) trees: Vec<crate::worktree::Status>,
    pub(super) view: Option<View>,
    pub(super) recent: Vec<String>,
    tools: HashMap<String, usize>,
    pub(super) usage: Vec<Round>,
    pub(super) prompt_tokens: u64,
    pub(super) completion_tokens: u64,
    pub(super) reasoning_tokens: u64,
    pub(super) cached_tokens: u64,
    pub(super) rounds: usize,
    pub(super) spinner: usize,
    pub(super) busy_since: Option<(Activity, Instant)>,
    pub(super) live_tool: Option<(String, String)>,
    pub(super) prefs: Prefs,
    pub(super) snapshot: Snapshot,
    pub(super) model: String,
    pub(super) relay: Option<String>,
    pub(super) peers: usize,
}

impl Model {
    pub(super) fn is_model(&self, choice: &str) -> bool {
        match choice.split_once('\t') {
            Some((endpoint, name)) => {
                endpoint == self.snapshot.endpoint && name == self.snapshot.model
            }
            None => choice == self.model,
        }
    }

    pub(super) fn new(info: &Info) -> Self {
        Self {
            session: info.session.clone(),
            entries: Vec::new(),
            ledger: Vec::new(),
            files: Vec::new(),
            diff: crate::diff::Diff::default(),
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
            rounds: 0,
            spinner: 0,
            busy_since: None,
            live_tool: None,
            prefs: info.prefs.clone(),
            snapshot: Snapshot {
                model: info.model.clone(),
                relay: info.relay.clone(),
                plan: info.plan,
                strict: info.strict,
                endpoint: info.endpoint.clone(),
                ..Snapshot::default()
            },
            model: info.model.clone(),
            relay: info.relay.clone(),
            peers: 0,
        }
    }

    pub(super) fn apply(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Session(id) => self.session = id,
            Msg::User(text) => self.entries.push(Entry::User(text)),
            Msg::Image(image) => self.entries.push(match preview(&image.bytes) {
                Ok(source) => Entry::Image {
                    label: format!("Attached image · {}", image.media_type),
                    source,
                },
                Err(error) => Entry::Error(format!("cannot show attached image: {error}")),
            }),
            Msg::Receipt(text) => self.receipt(&text),
            Msg::Bye => return false,
            Msg::Thinking(_) if !self.prefs.reasoning => {
                self.live_tool = Some(("thinking".to_string(), String::new()));
            }
            Msg::Thinking(text) => match self.entries.last_mut() {
                Some(Entry::Thinking(body)) => body.push_str(&text),
                _ => self.entries.push(Entry::Thinking(text)),
            },
            Msg::Delta(text) => {
                self.live_tool = None;
                match self.entries.last_mut() {
                    Some(Entry::Assistant {
                        text: body,
                        done: false,
                    }) => body.push_str(&text),
                    _ => self.entries.push(Entry::Assistant { text, done: false }),
                }
            }
            Msg::Calling(name) => self.live_tool = Some((name, "…".to_string())),
            Msg::Event(event) => self.apply_event(event),
            Msg::Busy(busy) => self.set_busy(busy),
            Msg::Failed(text) => {
                self.set_busy(None);
                self.entries.push(Entry::Error(text));
            }
            Msg::Restored(_) => {}
            Msg::System(text) => self.entries.push(Entry::System(text)),
            Msg::Cleared => self.clear(),
            Msg::Reset => {
                self.entries.clear();
                self.tools.clear();
                self.ledger.clear();
                self.recent.clear();
                self.usage.clear();
                self.prompt_tokens = 0;
                self.completion_tokens = 0;
                self.reasoning_tokens = 0;
                self.cached_tokens = 0;
                self.rounds = 0;
                self.live_tool = None;
            }
            Msg::Mode(plan) => self.snapshot.plan = plan,
            Msg::OpenSettings => {}
            Msg::Settings(snapshot) => self.snapshot = snapshot,
            Msg::Model { model, relay } => {
                self.model = model;
                self.relay = relay;
            }
            Msg::Models { .. }
            | Msg::ModelsFailed(_)
            | Msg::Sessions(_)
            | Msg::Prompts { .. }
            | Msg::Workflow { .. } => {}
            Msg::Mail { from, text } => self.entries.push(Entry::Mail { from, text }),
            Msg::Peers(peers) => self.peers = peers,
            Msg::Files(files) => self.files = files,
            Msg::Diff(diff) => self.diff = diff,
            Msg::Trees(trees) => self.trees = trees,
            Msg::Changed(_) => {}
            Msg::Edited(paths) => self.recent = paths,
            Msg::View {
                path,
                file,
                rope,
                added,
            } => {
                self.view = Some(View {
                    path,
                    file,
                    rope,
                    added,
                })
            }
            Msg::Saved { .. } => {}
            Msg::Patch { path, text } => self.patch = Some((path, text)),
            Msg::Resume(_) => {}
        }
        true
    }

    fn set_busy(&mut self, activity: Option<Activity>) {
        self.busy_since = activity.map(|activity| (activity, Instant::now()));
        if activity.is_some() {
            self.rounds = 0;
            self.spinner = 0;
        } else {
            self.live_tool = None;
        }
    }

    fn receipt(&mut self, text: &str) {
        let receipt = match self.entries.last() {
            Some(Entry::Assistant { text: body, .. }) => {
                text.strip_prefix(body).map(str::trim_start).unwrap_or(text)
            }
            _ => text,
        };
        if !receipt.is_empty() {
            self.entries.push(Entry::System(receipt.to_string()));
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.tools.clear();
        self.entries
            .push(Entry::System("history cleared".to_string()));
    }

    fn apply_event(&mut self, event: Event) {
        match event {
            Event::ToolCall {
                id,
                name,
                arguments,
            } => self.start_tool(id, name, arguments),
            Event::ToolResult {
                id,
                success,
                output,
                ..
            } => {
                self.finish_tool(&id, success, output);
            }
            Event::Assistant { text } => self.finish_assistant(text),
            Event::Aborted { reason } => self.entries.push(Entry::Aborted(reason)),
            Event::Merged {
                branch,
                base,
                files,
            } => self.entries.push(Entry::System(format!(
                "merged {branch} into {base}: {} file{}",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            ))),
            Event::Mail { from, text } => self.entries.push(Entry::Mail { from, text }),
            Event::Note { text } => self.entries.push(Entry::System(text)),
            Event::Verify { command, passed } => {
                self.entries.push(Entry::Verify { command, passed });
            }
            Event::Receipt { receipt } => self.entries.push(Entry::System(receipt.text)),
            Event::Work { progress } => {
                self.ledger = progress.ledger.clone();
                self.entries.push(Entry::System(progress.to_string()));
                if !progress.ledger.is_empty() {
                    self.entries
                        .push(Entry::System(progress.ledger.join("\n> ")));
                }
            }
            Event::Usage {
                prompt_tokens,
                completion_tokens,
                reasoning_tokens,
                cached_tokens,
                millis: _,
                prefix_hash,
                cost,
            } => self.record_usage(
                prompt_tokens,
                completion_tokens,
                reasoning_tokens,
                cached_tokens,
                prefix_hash,
                cost,
            ),
            Event::Delta { .. } | Event::Reasoning { .. } => {}
            Event::Child { id, event } => self.apply_child(&id, *event),
        }
    }

    fn apply_child(&mut self, id: &str, event: Event) {
        let tag: String = id.chars().take(8).collect();
        let line = match event {
            Event::ToolCall {
                name, arguments, ..
            } => {
                format!("child {tag} → {name} {}", tool_arg(&arguments))
            }
            Event::ToolResult {
                success: false,
                output,
                ..
            } => {
                format!("child {tag} ✗ {}", ToolOutput::Compact.text(&output))
            }
            Event::Assistant { text } => {
                format!("child {tag}: {}", ToolOutput::Compact.text(&text))
            }
            Event::Note { text } | Event::Aborted { reason: text } => {
                format!("child {tag}: {text}")
            }
            Event::Verify { command, passed } => {
                format!("child {tag} {} {command}", if passed { "✓" } else { "✗" })
            }
            _ => return,
        };
        self.entries.push(Entry::System(line));
    }

    fn start_tool(&mut self, id: String, name: String, arguments: String) {
        let arg = tool_arg(&arguments);
        self.tools.insert(id, self.entries.len());
        self.entries.push(Entry::Tool {
            name: name.clone(),
            arg: arg.clone(),
            state: ToolState::Running,
            output: String::new(),
        });
        self.live_tool = Some((name, arg));
    }

    fn finish_tool(&mut self, id: &str, success: bool, output: String) {
        if let Some(index) = self.tools.remove(id)
            && let Some(Entry::Tool {
                state,
                output: stored,
                ..
            }) = self.entries.get_mut(index)
        {
            *state = if success {
                ToolState::Ok
            } else {
                ToolState::Failed
            };
            *stored = output;
        }
        self.live_tool = None;
    }

    fn finish_assistant(&mut self, text: String) {
        let pending = self
            .entries
            .iter_mut()
            .rev()
            .take_while(|entry| {
                matches!(
                    entry,
                    Entry::Assistant { done: false, .. } | Entry::Verify { .. } | Entry::System(_)
                )
            })
            .find_map(|entry| match entry {
                Entry::Assistant { text, done } if !*done => Some((text, done)),
                _ => None,
            });
        if let Some((body, done)) = pending {
            *body = text;
            *done = true;
        } else {
            self.entries.push(Entry::Assistant { text, done: true });
        }
    }

    fn record_usage(
        &mut self,
        prompt: u32,
        completion: u32,
        reasoning: u32,
        cached: u32,
        prefix_hash: String,
        cost: Option<f64>,
    ) {
        self.rounds += 1;
        self.usage.push(Round {
            prompt,
            completion,
            reasoning,
            cached,
            prefix_hash,
            cost,
        });
        self.prompt_tokens += u64::from(prompt);
        self.completion_tokens += u64::from(completion);
        self.reasoning_tokens += u64::from(reasoning);
        self.cached_tokens += u64::from(cached);
    }

    pub(super) fn transcript(&self) -> String {
        self.entries
            .iter()
            .map(entry_markdown)
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

pub(super) fn entry_markdown(entry: &Entry) -> String {
    match entry {
        Entry::User(text) => format!("### You\n{text}"),
        Entry::Thinking(text) => format!("> **Thinking**\n> {}", text.replace('\n', "\n> ")),
        Entry::Assistant { text, .. } => text.clone(),
        Entry::Tool {
            name,
            arg,
            state,
            output,
        } => tool_markdown(name, arg, state, output),
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

pub(super) fn preview(bytes: &[u8]) -> Result<ImageSource, String> {
    let image = load_from_memory(bytes).map_err(|error| format!("invalid image: {error}"))?;
    let rgba = image
        .thumbnail(image.width().min(560), image.height().min(336))
        .into_rgba8();
    ImageSource::from_rgba8(rgba.width(), rgba.height(), rgba.into_raw())
        .ok_or_else(|| "image has no pixels".to_string())
}

fn tool_markdown(name: &str, arg: &str, state: &ToolState, output: &str) -> String {
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
