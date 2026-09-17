use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::time::Instant;

use image::load_from_memory;
use sigil::prelude::ImageSource;

use crate::prefs::{Prefs, Reasoning, ToolOutput};
use crate::tools::Event;

use super::super::{Activity, Info, Msg, Round, Snapshot, ToolState, tool_arg};
use super::speed::Speed;

#[derive(Clone, Debug)]
pub(super) enum Entry {
    User(String),
    Thinking(String),
    Receipt(String),
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
    #[expect(
        dead_code,
        reason = "nothing constructs this: the arms are unreachable, tracked in docs/api/AUDIT.md"
    )]
    Chart(Vec<Round>),
    Image {
        source: ImageSource,
        label: String,
    },
}

#[derive(Clone, Debug)]
pub(super) struct Agent {
    pub(super) id: String,
    pub(super) last: String,
    pub(super) done: bool,
    pub(super) failed: bool,
}

#[derive(Clone, Default)]
pub(super) struct Entries {
    inner: Vec<Entry>,
    pub(super) stamp: u64,
}

impl Entries {
    pub(super) fn invalidate(&mut self) {
        self.stamp = self.stamp.wrapping_add(1);
    }

    pub(super) fn push(&mut self, entry: Entry) {
        self.invalidate();
        self.inner.push(entry);
    }

    pub(super) fn clear(&mut self) {
        self.invalidate();
        self.inner.clear();
    }
}

impl Deref for Entries {
    type Target = [Entry];

    fn deref(&self) -> &[Entry] {
        &self.inner
    }
}

impl DerefMut for Entries {
    fn deref_mut(&mut self) -> &mut [Entry] {
        self.invalidate();
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
        self.invalidate();
        self.inner.extend(iter);
    }
}

#[derive(Clone)]
pub(super) struct View {
    pub(super) path: String,
    pub(super) file: std::path::PathBuf,
    pub(super) rope: sigil_ide_core::buffer::Rope,
    pub(super) added: Vec<(usize, usize)>,
    pub(super) image: Option<ImageSource>,
}

#[derive(Clone)]
pub(super) struct Model {
    pub(super) session: String,
    pub(super) entries: Entries,
    pub(super) ledger: Vec<String>,
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
            Msg::Image(image) => self.entries.push(match preview(&image.bytes) {
                Ok(source) => Entry::Image {
                    label: format!("Attached image · {}", image.media_type),
                    source,
                },
                Err(error) => Entry::Error(format!("cannot show attached image: {error}")),
            }),
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
            Msg::Models { .. } | Msg::Sessions(_) | Msg::Usage(_) | Msg::Prompts { .. } | Msg::Workflow { .. } => {}
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
                rope,
                added,
                image,
            } => {
                self.view = Some(View {
                    path,
                    file,
                    rope,
                    added,
                    image: image.and_then(|bytes| preview(&bytes).ok()),
                })
            }
            Msg::Saved { .. } => {}
            Msg::Patch { path, result, .. } => self.patch = Some((path, result)),
            Msg::Resume(_) => {}
        }
        true
    }

    fn set_busy(&mut self, activity: Option<Activity>) {
        self.busy_since = activity.map(|activity| (activity, Instant::now()));
        if activity == Some(Activity::Turn) {
            self.live_tool = Some(("thinking".to_string(), String::new()));
            self.speed.start(Instant::now());
        }
        if activity.is_some() {
            self.rounds = 0;
            self.spinner = 0;
        } else {
            self.live_tool = None;
            self.agents.retain(|agent| !agent.done);
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
            self.entries.push(Entry::Receipt(receipt.to_string()));
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.tools.clear();
        self.agents.clear();
        self.ledger.clear();
        self.plan_current = None;
        self.plan_total = 0;
        self.plan_objective.clear();
        self.entries
            .push(Entry::System("history cleared".to_string()));
    }

    fn apply_event(&mut self, event: Event) {
        match event {
            Event::Reasoning { text } if self.prefs.reasoning == Reasoning::Off => {
                self.speed.delta(text.chars().count(), Instant::now());
                self.live_tool = Some(("thinking".to_string(), String::new()));
            }
            Event::Reasoning { text } => {
                self.speed.delta(text.chars().count(), Instant::now());
                match self.entries.last_mut() {
                    Some(Entry::Thinking(body)) => body.push_str(&text),
                    _ => self.entries.push(Entry::Thinking(text)),
                }
            }
            Event::Delta { text } => {
                self.speed.delta(text.chars().count(), Instant::now());
                self.live_tool = None;
                match self.entries.last_mut() {
                    Some(Entry::Assistant {
                        text: body,
                        done: false,
                    }) => body.push_str(&text),
                    _ => self.entries.push(Entry::Assistant { text, done: false }),
                }
            }
            Event::ToolCall {
                id,
                name,
                arguments,
            } => {
                let shown = match name.as_str() {
                    "show_image" => Some((arguments.clone(), false)),
                    "read" | "read_file" => Some((arguments.clone(), true)),
                    _ => None,
                };
                if name == "todo" {
                    self.apply_todos(&arguments, None);
                }
                self.start_tool(id, name, arguments);
                if let Some((arguments, quiet)) = shown {
                    self.show_call(&arguments, quiet);
                }
            }
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
            Event::Receipt { receipt } => self.entries.push(Entry::Receipt(receipt.text)),
            Event::Work { progress } => {
                self.ledger = progress
                    .ledger
                    .iter()
                    .filter(|line| itemized(line))
                    .cloned()
                    .collect();
                self.plan_current = progress.current;
                self.plan_total = progress.total;
                self.plan_objective = progress.objective.clone();
                if progress.kind != crate::WorkKind::Single || progress.reason.is_some() {
                    self.entries.push(Entry::System(progress.to_string()));
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
            Event::Approval { .. } => {}
            Event::Unreported { tokens } => self.unreported_tokens = tokens,
            Event::Child { id, event } => self.apply_child(&id, *event),
        }
    }

    fn apply_todos(&mut self, arguments: &str, child: Option<&str>) {
        let Ok(items) = crate::tools::todos_from_args(arguments) else {
            return;
        };
        if items.is_empty() && child.is_none() {
            self.ledger.clear();
            self.entries.invalidate();
            return;
        }
        let lines: Vec<String> = items
            .iter()
            .map(|todo| match child {
                Some(tag) => format!("  {} {tag} · {}", todo.mark(), todo.text),
                None => todo.ledger_line(),
            })
            .collect();
        if let Some(tag) = child {
            let needle = format!(" {tag} · ");
            self.ledger.retain(|line| !line.contains(&needle));
            self.ledger.extend(lines);
        } else {
            self.ledger = lines;
        }
        self.entries.invalidate();
    }

    fn apply_child(&mut self, id: &str, event: Event) {
        if let Event::ToolCall {
            name, arguments, ..
        } = &event
            && name == "todo"
        {
            let tag: String = id.chars().take(8).collect();
            self.apply_todos(arguments, Some(&tag));
        }
        let tag: String = id.chars().take(8).collect();
        let compact = ToolOutput::Compact;
        let (last, done, failed) = match event {
            Event::ToolCall {
                name, arguments, ..
            } => (
                format!("{name} {}", tool_arg(&arguments))
                    .trim()
                    .to_string(),
                false,
                false,
            ),
            Event::ToolResult {
                success: false,
                output,
                ..
            } => (format!("✗ {}", compact.text(&output)), false, true),
            Event::Assistant { text } => (compact.text(&text).to_string(), true, false),
            Event::Aborted { reason } => (reason, true, true),
            Event::Note { text } => (text, false, false),
            Event::Verify { command, passed } => (
                format!("{} {command}", if passed { "✓" } else { "✗" }),
                false,
                !passed,
            ),
            _ => return,
        };
        if let Some(agent) = self.agents.iter_mut().find(|agent| agent.id == tag) {
            agent.last = last.clone();
            agent.done = done;
            agent.failed |= failed;
        } else {
            self.agents.push(Agent {
                id: tag.clone(),
                last: last.clone(),
                done,
                failed,
            });
            self.agents_hidden = false;
        }
        self.entries.invalidate();
        if done {
            let failed = self
                .agents
                .iter()
                .find(|agent| agent.id == tag)
                .is_some_and(|agent| agent.failed);
            self.entries.push(Entry::Tool {
                name: "agent".to_string(),
                arg: tag,
                state: if failed {
                    ToolState::Failed
                } else {
                    ToolState::Ok
                },
                output: last,
            });
        }
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
        self.live_tool = Some(("thinking".to_string(), String::new()));
    }

    fn finish_assistant(&mut self, text: String) {
        let links = linked(&text);
        self.place_assistant(text);
        for path in links {
            self.show(&path);
        }
    }

    fn show_call(&mut self, arguments: &str, quiet: bool) {
        let path = serde_json::from_str::<serde_json::Value>(arguments)
            .ok()
            .and_then(|value| value.get("path")?.as_str().map(str::to_string));
        if let Some(path) = path {
            self.show_path(&path, quiet);
        }
    }

    fn show(&mut self, path: &str) {
        self.show_path(path, false);
    }

    fn show_path(&mut self, path: &str, quiet: bool) {
        let full = if std::path::Path::new(path).is_absolute() {
            std::path::PathBuf::from(path)
        } else {
            std::env::current_dir()
                .map(|dir| dir.join(path))
                .unwrap_or_else(|_| std::path::PathBuf::from(path))
        };
        let bytes = std::fs::read(&full).map_err(|error| error.to_string());
        if quiet
            && !bytes
                .as_ref()
                .is_ok_and(|bytes| crate::tools::image_type(bytes).is_some())
        {
            return;
        }
        let entry = match bytes {
            Ok(bytes) => match preview(&bytes) {
                Ok(source) => Entry::Image {
                    source,
                    label: path.to_string(),
                },
                Err(error) => Entry::Error(format!("{path}: {error}")),
            },
            Err(error) => Entry::Error(format!("{path}: {error}")),
        };
        self.entries.push(entry);
    }

    fn place_assistant(&mut self, text: String) {
        let pending = self
            .entries
            .iter_mut()
            .rev()
            .take_while(|entry| {
                matches!(
                    entry,
                    Entry::Assistant { done: false, .. }
                        | Entry::Verify { .. }
                        | Entry::Receipt(_)
                        | Entry::System(_)
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
        self.speed.finish(completion.max(reasoning), Instant::now());
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
        Entry::Receipt(text) => text.clone(),
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

fn linked(text: &str) -> Vec<String> {
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

pub(super) fn itemized(line: &str) -> bool {
    let text = line.trim_start();
    text.starts_with("[x] ")
        || text.starts_with("[/] ")
        || text.starts_with("[ ] ")
        || text.starts_with("[!] ")
        || text.chars().next().is_some_and(|c| c.is_ascii_digit()) && text.contains(". ")
}

const PREVIEW_WIDTH: u32 = 1920;
const PREVIEW_HEIGHT: u32 = 1200;

pub(super) fn preview(bytes: &[u8]) -> Result<ImageSource, String> {
    let image = load_from_memory(bytes).map_err(|error| format!("invalid image: {error}"))?;
    let rgba = image
        .thumbnail(
            image.width().min(PREVIEW_WIDTH),
            image.height().min(PREVIEW_HEIGHT),
        )
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
