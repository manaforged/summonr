use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
    program: String,
    env: Vec<(String, String)>,
    runs: Arc<Mutex<HashMap<String, Run>>>,
    threads: Arc<Mutex<HashMap<String, String>>>,
}

impl Seat {
    pub(super) fn new(program: &str, env: &std::collections::BTreeMap<String, String>) -> Self {
        Self {
            program: program.to_string(),
            env: env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            runs: Arc::default(),
            threads: Arc::default(),
        }
    }

    fn take(&self, session: &str) -> Option<Run> {
        self.runs.lock().ok()?.remove(session)
    }

    fn keep(&self, session: &str, run: Run) {
        if let Ok(mut runs) = self.runs.lock() {
            runs.insert(session.to_string(), run);
        }
    }

    fn forget(&self, session: &str) {
        if let Ok(mut threads) = self.threads.lock() {
            threads.remove(session);
        }
    }

    fn thread(&self, session: &str) -> Option<String> {
        self.threads.lock().ok()?.get(session).cloned()
    }

    fn remember(&self, session: &str, id: &str) {
        if let Ok(mut threads) = self.threads.lock() {
            threads.insert(session.to_string(), id.to_string());
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
        let step = self.turn(seat, request, partial).await;
        if step.is_err() {
            seat.forget(&request.session);
        }
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
            .filter(|run| !run.abandoned(request.messages));
        let mut run = match held {
            Some(mut run) => {
                run.answer(request.messages).await?;
                run
            }
            None => {
                let known = seat.thread(&request.session);
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
            seat.remember(&request.session, id);
        }
        match step? {
            Step::Paused(message) => {
                seat.keep(&request.session, run);
                Ok(message)
            }
            Step::Done(message) => Ok(message),
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

    pub(super) async fn agent_models(&self) -> Result<Vec<crate::catalog::Model>> {
        run::models(self.seat()?).await
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
