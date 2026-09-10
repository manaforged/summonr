use crate::durable::log::{self, Line, Usage};
use crate::session::user_text;
use crate::tools::Event;

use super::{Msg, Worker};

impl Worker {
    pub(super) fn replay(&self) {
        let lines = match log::read(self.loop_.session_path()) {
            Ok(lines) => lines,
            Err(error) => {
                self.system(&format!("cannot load conversation: {error}"));
                return;
            }
        };
        let charged = lines
            .iter()
            .any(|line| matches!(line, Line::Assistant { .. } | Line::Charge { .. }));
        for line in lines {
            self.restore(line);
        }
        if charged {
            self.system(
                "Restored token usage. Historical reasoning tokens and cost are unavailable.",
            );
        }
    }

    fn restore(&self, line: Line) {
        match line {
            Line::User {
                text,
                source,
                images,
                start,
                ..
            } => {
                let text = source.unwrap_or_else(|| user_text(&text).to_string());
                let message = if start {
                    Msg::User(text)
                } else {
                    Msg::System(text)
                };
                drop(self.ui.send(message));
                for image in images {
                    drop(self.ui.send(Msg::Image(image)));
                }
            }
            Line::Assistant {
                text,
                reasoning,
                tool_calls,
                usage,
                ..
            } => {
                if !reasoning.is_empty() {
                    drop(self.ui.send(Msg::Thinking(reasoning)));
                }
                if !text.is_empty() {
                    drop(self.ui.send(Msg::Event(Event::Assistant { text })));
                }
                for call in tool_calls {
                    drop(self.ui.send(Msg::Event(Event::ToolCall {
                        id: call.id,
                        name: call.function.name,
                        arguments: call.function.arguments,
                    })));
                }
                self.usage(usage, String::new());
            }
            Line::Charge { usage, note, model } => self.usage(usage, format!("{note} · {model}")),
            Line::Tool {
                call,
                text,
                success,
                ..
            } => {
                drop(self.ui.send(Msg::Event(Event::ToolResult {
                    id: call,
                    output: text,
                    success,
                    mutated: false,
                })));
            }
            Line::Receipt { text } => drop(self.ui.send(Msg::Receipt(text))),
            Line::Clear => drop(self.ui.send(Msg::Cleared)),
            Line::Close {
                reason: Some(reason),
            } => {
                drop(self.ui.send(Msg::Event(Event::Aborted { reason })));
            }
            _ => {}
        }
    }

    fn usage(&self, usage: Usage, prefix_hash: String) {
        drop(self.ui.send(Msg::Event(Event::Usage {
            prompt_tokens: u32::try_from(usage.input).unwrap_or(u32::MAX),
            completion_tokens: u32::try_from(usage.output).unwrap_or(u32::MAX),
            cached_tokens: u32::try_from(usage.cached).unwrap_or(u32::MAX),
            reasoning_tokens: 0,
            millis: Some(usage.millis),
            prefix_hash,
            cost: None,
        })));
    }
}
