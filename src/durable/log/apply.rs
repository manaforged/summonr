use super::*;

impl State {
    pub(crate) fn apply(&mut self, line: Line) {
        match line {
            Line::Session {
                id,
                model,
                system,
                workspace,
                parent,
                depth,
            } => {
                self.id = Some(id);
                self.model = model;
                self.system = system;
                self.workspace = workspace;
                self.parent = parent;
                self.depth = depth;
            }
            Line::Tools { .. } | Line::Receipt { .. } => {}
            Line::Budget { tokens, millis } => {
                self.tokens = tokens;
                self.millis = millis;
            }
            Line::Workflow { name } => self.workflow = name,
            Line::Child { record } => {
                self.children.insert(record.id, record);
            }
            Line::User {
                text,
                images,
                start,
                ..
            } => self.apply_user(text, images, start),
            Line::Assistant {
                text,
                reasoning,
                tool_calls,
                usage,
                continuation,
                ..
            } => self.apply_assistant(text, reasoning, tool_calls, usage, continuation),
            Line::Interrupted { text, .. } => {
                self.history.extend((!text.is_empty()).then(|| Message {
                    role: Role::Assistant,
                    ..Message::system(text)
                }));
            }
            Line::Tool {
                call,
                text,
                success,
                millis,
                ..
            } => self.apply_tool(call, text, success, millis),
            Line::Charge {
                usage, reported, ..
            } => {
                if !reported {
                    self.unreported = self.unreported.saturating_add(usage.tokens());
                }
                self.charge(usage);
            }
            Line::Summary { text, tail } => self.apply_summary(text, &tail),
            Line::Rewrite { mut edits } => {
                edits.retain(|(index, _)| *index < self.history.len());
                for (index, message) in edits {
                    self.history[index] = message;
                }
            }
            end @ (Line::Clear | Line::Close { .. }) => self.apply_end(&end),
        }
    }

    fn apply_user(&mut self, text: String, images: Vec<Image>, start: bool) {
        if start {
            self.opener = Some(self.history.len());
            self.unanswered.clear();
        }
        let mut message = Message::user(text);
        message.images = images;
        self.history.push(message);
    }

    fn apply_assistant(
        &mut self,
        text: String,
        reasoning: String,
        mut tool_calls: Vec<ToolCall>,
        usage: Usage,
        continuation: Option<Continuation>,
    ) {
        self.charge(usage);
        for call in &mut tool_calls {
            call.function.check();
        }
        self.unanswered = tool_calls.iter().map(|call| call.id.clone()).collect();
        self.history.push(Message {
            role: Role::Assistant,
            tool_calls,
            reasoning: (!reasoning.is_empty()).then_some(reasoning),
            continuation,
            ..Message::system(text)
        });
    }

    fn apply_tool(&mut self, call: String, text: String, success: bool, millis: u64) {
        self.elapsed = self.elapsed.saturating_add(millis);
        self.unanswered.retain(|id| *id != call);
        self.history.push(Message {
            failed: !success,
            ..Message::tool(call, text)
        });
    }

    fn apply_summary(&mut self, text: String, tail: &[Message]) {
        let opener = self
            .opener
            .and_then(|index| self.history.get(index).cloned());
        self.history.clear();
        self.opener = opener.as_ref().map(|_| 0);
        self.history.extend(opener.clone());
        self.history.push(Message::system(text));
        let repeated = opener
            .as_ref()
            .is_some_and(|opener| tail.first() == Some(opener));
        self.history
            .extend(tail.iter().skip(usize::from(repeated)).cloned());
    }

    fn apply_end(&mut self, end: &Line) {
        if let Line::Close { reason, .. } = end {
            self.last_close_reason = reason.clone();
        }
        if *end == Line::Clear {
            self.history.clear();
            self.last_close_reason = None;
        }
        self.opener = None;
        self.unanswered.clear();
    }
}
