use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::{AssistantMessage, Client, CompletionRequest, Message, Partial, Role};
use crate::error::{Error, Result};

mod bridge;
mod frame;
mod run;
mod step;

use run::{Run, Thread};
use step::Step;

#[derive(Clone)]
pub(super) struct Seat {
    name: String,
    program: String,
    env: Vec<(String, String)>,
    unset: &'static [&'static str],
    runs: Arc<Mutex<HashMap<String, (Instant, Run)>>>,
    threads: Arc<Mutex<HashMap<String, Known>>>,
    gates: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

#[derive(Clone)]
struct Known {
    id: String,
    seen: Seen,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Seen {
    count: usize,
    digest: u64,
}

impl Seen {
    fn of(messages: &[Message]) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let spoken: Vec<&Message> = messages
            .iter()
            .filter(|message| message.role != Role::System)
            .collect();
        for message in &spoken {
            std::hash::Hash::hash(&transcript_line(message), &mut hasher);
        }
        Self {
            count: spoken.len(),
            digest: std::hash::Hasher::finish(&hasher),
        }
    }

    fn extended_by(self, messages: &[Message]) -> bool {
        let spoken: Vec<Message> = messages
            .iter()
            .filter(|message| message.role != Role::System)
            .take(self.count)
            .cloned()
            .collect();
        spoken.len() == self.count && Self::of(&spoken) == self
    }
}

struct Forget<'a> {
    seat: &'a Seat,
    session: &'a str,
    armed: bool,
}

impl Drop for Forget<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.seat.forget(self.session);
        }
    }
}

impl Seat {
    pub(super) fn new(endpoint: &crate::Endpoint, program: &str) -> Self {
        Self {
            name: endpoint.name.clone(),
            program: program.to_string(),
            env: endpoint
                .env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            unset: crate::endpoint::env_unset(&endpoint.name),
            runs: Arc::default(),
            threads: Arc::default(),
            gates: Arc::default(),
        }
    }

    fn take(&self, session: &str) -> Option<Run> {
        self.runs.lock().ok()?.remove(session).map(|(_, run)| run)
    }

    fn keep(&self, session: &str, run: Run) {
        if let Ok(mut runs) = self.runs.lock() {
            let limit = crate::limits::limits().agent_park();
            runs.retain(|_, (parked, _)| parked.elapsed() < limit);
            runs.insert(session.to_string(), (Instant::now(), run));
        }
    }

    fn reusable(&self, mut run: Run, request: &CompletionRequest<'_>) -> Option<Run> {
        if !run.fits(request) {
            return None;
        }
        if !run.pending.is_empty() {
            return Some(run);
        }
        let asked = request
            .messages
            .last()
            .is_some_and(|message| message.role == Role::User);
        (asked && self.grown(&request.session, request.messages)).then_some(run)
    }

    fn grown(&self, session: &str, messages: &[Message]) -> bool {
        let Some(seen) = self
            .threads
            .lock()
            .ok()
            .and_then(|threads| threads.get(session).map(|known| known.seen))
        else {
            return false;
        };
        let spoken = messages
            .iter()
            .filter(|message| message.role != Role::System)
            .count();
        spoken > seen.count && seen.extended_by(messages)
    }

    fn gate(&self, session: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.gates.lock().map_or_else(
            |_| Arc::default(),
            |mut gates| Arc::clone(gates.entry(session.to_string()).or_default()),
        )
    }

    fn forget(&self, session: &str) {
        if let Ok(mut threads) = self.threads.lock() {
            threads.remove(session);
        }
    }

    fn thread(&self, session: &str, messages: &[Message]) -> Option<String> {
        let known = self.threads.lock().ok()?.get(session).cloned()?;
        if known.seen.extended_by(messages) {
            return Some(known.id);
        }
        self.forget(session);
        None
    }

    fn remember(&self, session: &str, id: &str, messages: &[Message]) {
        if let Ok(mut threads) = self.threads.lock() {
            threads.insert(
                session.to_string(),
                Known {
                    id: id.to_string(),
                    seen: Seen::of(messages),
                },
            );
        }
    }
}

impl Client {
    fn seat(&self) -> Result<&Seat> {
        self.agent
            .as_ref()
            .ok_or_else(|| Error::Config("this endpoint has no Claude Code seat".to_string()))
    }

    pub(super) async fn stream_agent(
        &self,
        request: &CompletionRequest<'_>,
        partial: &mut Partial,
    ) -> Result<AssistantMessage> {
        let seat = self.seat()?;
        let gate = seat.gate(&request.session);
        let _turn = gate.lock().await;
        let mut forget = Forget {
            seat,
            session: &request.session,
            armed: true,
        };
        let step = self.turn(seat, request, partial).await;
        forget.armed = step.is_err();
        step
    }

    async fn turn(
        &self,
        seat: &Seat,
        request: &CompletionRequest<'_>,
        partial: &mut Partial,
    ) -> Result<AssistantMessage> {
        let held = seat
            .take(&request.session)
            .filter(|run| !run.abandoned(request.messages))
            .and_then(|run| seat.reusable(run, request));
        let mut run = match held {
            Some(mut run) => {
                run.answer(request.messages).await?;
                run
            }
            None => {
                let known = seat.thread(&request.session, request.messages);
                let thread = match &known {
                    Some(id) => Thread::Resume(id.clone()),
                    None => Thread::Start(uuid::Uuid::now_v7().to_string()),
                };
                let input = opening(request.messages, known.is_none());
                Run::start(seat, request, thread, &input).await?
            }
        };
        let step = run.next(self, request, partial).await;
        if let Some(id) = run.recorded() {
            seat.remember(&request.session, id, request.messages);
        }
        match step? {
            Step::Paused(message) | Step::Done(message) => {
                seat.keep(&request.session, run);
                Ok(message)
            }
        }
    }

    pub(super) async fn complete_agent(
        &self,
        request: &CompletionRequest<'_>,
    ) -> Result<AssistantMessage> {
        let seat = self.seat()?;
        let input = opening(request.messages, true);
        let mut run = Run::start(seat, request, Thread::Once, &input).await?;
        match run.next(self, request, &mut Partial::default()).await? {
            Step::Paused(message) | Step::Done(message) => Ok(message),
        }
    }
}

fn opening(messages: &[Message], replay: bool) -> Vec<Message> {
    let start = messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    let split = messages
        .iter()
        .rposition(|message| message.role != Role::User)
        .map_or(0, |at| at + 1)
        .max(start);
    let mut out = messages[split..].to_vec();
    let earlier = &messages[start..split];
    if !replay && out.is_empty() {
        let asked = messages
            .iter()
            .rposition(|message| message.role == Role::User)
            .unwrap_or(start);
        let tail: String = messages[asked..].iter().map(transcript_line).collect();
        return vec![Message::user(format!("{tail}\nContinue."))];
    }
    if !replay || earlier.is_empty() {
        return out;
    }
    let mut history = String::from("<earlier_conversation>\n");
    for message in earlier {
        history.push_str(&transcript_line(message));
    }
    history.push_str("</earlier_conversation>\n\n");
    match out.first_mut() {
        Some(first) => first.content.insert_str(0, &history),
        None => out.push(Message::user(format!("{history}Continue."))),
    }
    out
}

fn transcript_line(message: &Message) -> String {
    let who = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool result",
        Role::System => "system",
    };
    let mut line = format!("[{who}] {}\n", message.content.trim());
    for call in &message.tool_calls {
        line.push_str(&format!(
            "[tool call {}] {}\n",
            call.function.name, call.function.arguments
        ));
    }
    line
}

#[cfg(all(test, unix))]
#[path = "agent_tests.rs"]
mod tests;
