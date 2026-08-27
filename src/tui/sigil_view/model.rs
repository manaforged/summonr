use std::collections::HashMap;
use std::time::Instant;

use crate::prefs::{Prefs, ToolOutput};
use crate::tools::Event;

use super::super::{Info, Msg, Round, Snapshot, ToolState, last_line, tail_lines, tool_arg};

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
        tail: String,
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
        bytes: Vec<u8>,
        label: String,
    },
}

#[derive(Clone)]
pub(super) struct Model {
    pub(super) entries: Vec<Entry>,
    tools: HashMap<String, usize>,
    pub(super) usage: Vec<Round>,
    pub(super) prompt_tokens: u64,
    pub(super) completion_tokens: u64,
    pub(super) reasoning_tokens: u64,
    pub(super) cached_tokens: u64,
    pub(super) rounds: usize,
    pub(super) spinner: usize,
    pub(super) busy_since: Option<Instant>,
    pub(super) live_tool: Option<(String, String)>,
    pub(super) prefs: Prefs,
    pub(super) snapshot: Snapshot,
    pub(super) model: String,
    pub(super) relay: Option<String>,
    pub(super) peers: usize,
}

impl Model {
    pub(super) fn new(info: &Info) -> Self {
        Self {
            entries: Vec::new(),
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
            Msg::Bye => return false,
            // Reasoning display is off, but a reasoning-heavy model can think
            // for many seconds — show that it is alive without the transcript.
            Msg::Thinking(_) if !self.prefs.reasoning => {
                self.live_tool = Some(("thinking".to_string(), String::new()));
            }
            Msg::Thinking(text) => match self.entries.last_mut() {
                Some(Entry::Thinking(body)) => body.push_str(&text),
                _ => self.entries.push(Entry::Thinking(text)),
            },
            Msg::Delta(text) => {
                // The answer is streaming now; drop the thinking/calling hint.
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
                self.set_busy(false);
                self.entries.push(Entry::Error(text));
            }
            Msg::System(text) => self.entries.push(Entry::System(text)),
            Msg::Cleared => self.clear(),
            Msg::Mode(plan) => self.snapshot.plan = plan,
            Msg::OpenSettings => {}
            Msg::Settings(snapshot) => self.snapshot = snapshot,
            Msg::Model { model, relay } => {
                self.model = model;
                self.relay = relay;
            }
            Msg::Models { .. } | Msg::ModelsFailed(_) | Msg::Workflow { .. } => {}
            Msg::Mail { from, text } => self.entries.push(Entry::Mail { from, text }),
            Msg::Peers(peers) => self.peers = peers,
        }
        true
    }

    fn set_busy(&mut self, busy: bool) {
        self.busy_since = busy.then(Instant::now);
        if busy {
            self.rounds = 0;
            self.spinner = 0;
        } else {
            self.live_tool = None;
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
                self.finish_tool(&id, success, &output);
            }
            Event::Assistant { text } => self.finish_assistant(text),
            Event::Aborted { reason } => self.entries.push(Entry::Aborted(reason)),
            Event::Mail { from, text } => self.entries.push(Entry::Mail { from, text }),
            Event::Note { text } => self.entries.push(Entry::System(text)),
            Event::Verify { command, passed } => {
                self.entries.push(Entry::Verify { command, passed });
            }
            Event::Receipt { .. } => {}
            Event::Work { progress } => {
                self.entries.push(Entry::System(progress.to_string()));
            }
            Event::Usage {
                prompt_tokens,
                completion_tokens,
                reasoning_tokens,
                cached_tokens,
                prefix_hash,
            } => self.record_usage(
                prompt_tokens,
                completion_tokens,
                reasoning_tokens,
                cached_tokens,
                prefix_hash,
            ),
            Event::Delta { .. } => {}
        }
    }

    fn start_tool(&mut self, id: String, name: String, arguments: String) {
        let arg = tool_arg(&arguments);
        self.tools.insert(id, self.entries.len());
        self.entries.push(Entry::Tool {
            name: name.clone(),
            arg: arg.clone(),
            state: ToolState::Running,
            tail: String::new(),
        });
        self.live_tool = Some((name, arg));
    }

    fn finish_tool(&mut self, id: &str, success: bool, output: &str) {
        if let Some(index) = self.tools.remove(id)
            && let Some(Entry::Tool { state, tail, .. }) = self.entries.get_mut(index)
        {
            *state = if success {
                ToolState::Ok
            } else {
                ToolState::Failed
            };
            *tail = if self.prefs.tools == ToolOutput::Full {
                tail_lines(output, 12)
            } else {
                last_line(output)
            };
        }
        self.live_tool = None;
    }

    fn finish_assistant(&mut self, text: String) {
        match self.entries.last_mut() {
            Some(Entry::Assistant { text: body, done }) if !*done => {
                *body = text;
                *done = true;
            }
            _ => self.entries.push(Entry::Assistant { text, done: true }),
        }
    }

    fn record_usage(
        &mut self,
        prompt: u32,
        completion: u32,
        reasoning: u32,
        cached: u32,
        prefix_hash: String,
    ) {
        self.rounds += 1;
        self.usage.push(Round {
            prompt,
            completion,
            reasoning,
            cached,
            prefix_hash,
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

fn entry_markdown(entry: &Entry) -> String {
    match entry {
        Entry::User(text) => format!("### You\n{text}"),
        Entry::Thinking(text) => format!("> **Thinking**\n> {}", text.replace('\n', "\n> ")),
        Entry::Assistant { text, .. } => text.clone(),
        Entry::Tool {
            name,
            arg,
            state,
            tail,
        } => tool_markdown(name, arg, state, tail),
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

fn tool_markdown(name: &str, arg: &str, state: &ToolState, tail: &str) -> String {
    let mark = match state {
        ToolState::Running => "⏳",
        ToolState::Ok => "✓",
        ToolState::Failed => "✗",
    };
    if tail.is_empty() {
        format!("{mark} **{name}** `{arg}`")
    } else {
        format!("{mark} **{name}** `{arg}`\n```text\n{tail}\n```")
    }
}
