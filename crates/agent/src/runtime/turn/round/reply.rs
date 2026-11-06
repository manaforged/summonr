use std::collections::HashSet;

use super::super::close::Answer;
use super::super::repeat::call_key;
use super::*;
use crate::tools::Builtin;

#[derive(Default)]
pub(super) struct Pace {
    stalls: usize,
    seen: HashSet<u64>,
    planned: bool,
    acted: bool,
    judged: bool,
}

impl Pace {
    fn note_calls(&mut self, message: &Message) {
        let mut fresh = false;
        for call in &message.tool_calls {
            fresh |= self.seen.insert(call_key(&call.function));
        }
        if fresh {
            self.stalls = 0;
        }
        self.planned |= message
            .tool_calls
            .iter()
            .any(|call| Builtin::is(&call.function.name, Builtin::Todo));
        self.acted |= acts(message);
    }

    fn stalled(&self) -> bool {
        self.stalls >= crate::limits::limits().stall_rounds
    }
}

impl<P: Provider> Loop<P> {
    pub(super) async fn take_round(
        &mut self,
        reply: (Message, String, Attempt, Meter, UsageMeta),
        stop_reason: StopReason,
        pace: &mut Pace,
        (messages, pre, events): (&mut Vec<Message>, &mut Pretask, &mut Vec<Event>),
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Option<Message>> {
        let (message, reasoning, attempt, meter, meta) = reply;
        if !message.tool_calls.is_empty() {
            pace.note_calls(&message);
            let reply = (message, reasoning, attempt, meter, meta);
            self.continue_with(reply, messages, pre, events, emit, usage)
                .await?;
            return Ok(None);
        }
        if stop_reason == StopReason::Length {
            let mut steered = Vec::new();
            self.steer_length(
                &message,
                &reasoning,
                attempt,
                meter,
                meta,
                &mut steered,
                events,
                emit,
            )
            .await?;
            messages.push(message);
            messages.append(&mut steered);
            return Ok(None);
        }
        let reply = (message, reasoning, attempt, meter, meta);
        self.close_reply(reply, pace, (messages, pre, events), emit, usage)
            .await
    }

    async fn close_reply(
        &mut self,
        (message, reasoning, attempt, meter, meta): (Message, String, Attempt, Meter, UsageMeta),
        pace: &mut Pace,
        (messages, pre, events): (&mut Vec<Message>, &mut Pretask, &mut Vec<Event>),
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Option<Message>> {
        let stalled = pace.stalled();
        let steered = self.steer_waiting();
        let tasks = if steered {
            None
        } else {
            self.open_tasks(pace)?
        };
        let holds = !stalled && (steered || tasks.is_some());
        if !holds && message.content.trim().is_empty() {
            self.settle(pre, events, emit).await?;
            return self.reject_empty(attempt, meter.millis, events, emit);
        }
        let answer = Answer {
            attempt,
            text: &message.content,
            reasoning: &reasoning,
            meter,
            meta,
        };
        self.record_answer(answer, events, emit)?;
        if stalled && (steered || tasks.is_some() || self.judge_due(pace)) {
            self.note_stall(pace, events, emit)?;
        }
        let note = if stalled {
            None
        } else if holds {
            tasks
        } else {
            self.judge_reply(&message.content, pace, events, emit, usage)
                .await?
        };
        if !holds && note.is_none() {
            return Ok(Some(message));
        }
        self.keep_reply(message, messages, events, emit)?;
        if steered {
            let before = messages.len();
            self.take_mail(messages, events, emit)?;
            pace.stalls += usize::from(messages.len() == before);
        }
        if let Some(note) = note {
            self.continue_turn(note, pace, messages, events, emit)?;
        }
        Ok(None)
    }

    fn note_stall(
        &mut self,
        pace: &Pace,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let text = format!(
            "the turn ended: {} replies in a row made no new tool call; send a message to keep going",
            pace.stalls
        );
        self.push_event(events, emit, Event::Note { text })
    }

    fn continue_turn(
        &mut self,
        note: String,
        pace: &mut Pace,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        pace.stalls += 1;
        self.inject(note, messages)?;
        let text = "work is still open; the turn continues".to_string();
        self.push_event(events, emit, Event::Note { text })
    }

    fn keep_reply(
        &mut self,
        message: Message,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if !message.content.is_empty() {
            let text = message.content.clone();
            self.push_event(events, emit, Event::Assistant { text })?;
        }
        messages.push(message);
        Ok(())
    }

    fn open_tasks(&self, pace: &Pace) -> Result<Option<String>> {
        if self.session.stage() == Some(Stage::Plan) || !pace.planned {
            return Ok(None);
        }
        let todos = match self.session.work_progress() {
            Ok(progress) => progress.todos,
            Err(crate::durable::driver::DriverError::NoWork) => return Ok(None),
            Err(error) => return Err(Error::from(error)),
        };
        let open = todos
            .iter()
            .filter(|todo| todo.open())
            .map(crate::tools::Todo::line)
            .collect::<Vec<_>>();
        Ok((!open.is_empty()).then(|| format!("{OPEN_WORK}\n{}", open.join("\n"))))
    }

    fn judge_due(&self, pace: &Pace) -> bool {
        self.judge_turns
            && pace.acted
            && !pace.judged
            && !self.waits_on_user()
            && !self.is_goal()
            && self.session.stage() != Some(Stage::Plan)
    }

    fn waits_on_user(&self) -> bool {
        self.session.work_progress().is_ok_and(|progress| {
            progress
                .todos
                .iter()
                .any(|todo| todo.status == crate::tools::TodoStatus::Blocked)
        })
    }

    async fn judge_reply(
        &mut self,
        reply: &str,
        pace: &mut Pace,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Option<String>> {
        if !self.judge_due(pace) {
            return Ok(None);
        }
        let Ok(objective) = self.session.objective() else {
            return Ok(None);
        };
        let evidence = goal::with_corrections(TURN_EVIDENCE, events);
        let decision = goal::judged(self, &objective, reply, &evidence, usage).await;
        self.sync_usage(emit);
        match decision {
            Ok(Decision::GoalContinue(reason)) => {
                pace.judged = true;
                Ok(Some(format!("{UNFINISHED}\n{reason}")))
            }
            Ok(_) => Ok(None),
            Err(text) => {
                self.push_event(events, emit, Event::Note { text })?;
                Ok(None)
            }
        }
    }
}

fn acts(message: &Message) -> bool {
    message
        .tool_calls
        .iter()
        .any(|call| Builtin::of(&call.function.name).is_some_and(|tool| !tool.readonly()))
}

const OPEN_WORK: &str = "[summon control: the reply had no tool call, but tasks are still open. Continue with the next one now. Mark a task done when it is finished, or blocked when only the user or an outside event can move it, and name who in its text.]\nOpen tasks:";

const UNFINISHED: &str = "[summon control: the reply had no tool call, but the judge found work you can still do without the user. Continue it now. If only the user can unblock what is left, say so in one line.]\nWhat is left:";

const TURN_EVIDENCE: &str = "Interactive turn. Reply done when nothing is left that the agent can do without the user, including when the only remaining step needs the user's approval. Reply continue only when the agent stopped with work it can still do; name that work. Findings, proposals, plans and rewritten prompts the agent offered are not work the user asked for: when only those remain, reply done.";
