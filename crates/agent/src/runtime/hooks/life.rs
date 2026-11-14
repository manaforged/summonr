use super::late::Opening;
use super::*;
use crate::hooks::{End, Failure, Start};
use summonr_core::error::ProviderFailure;

impl<P: Provider> Loop<P> {
    pub(in crate::runtime) async fn admit(
        &mut self,
        input: &mut UserInput,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if self.turn_started == 0 {
            self.turn_started = summonr_core::clock::now();
        }
        let mut heard = self.take_opening().await;
        let prompt = input
            .source
            .as_ref()
            .map_or(&input.text, |source| &source.text)
            .clone();
        let payload = json!({ "prompt": prompt });
        heard.merge(self.hook(HookEvent::Prompt, None, payload).await);
        heard.context.extend(self.pending_hooks.take_late());
        for note in &heard.notes {
            emit(&note_event(HookEvent::Prompt, note));
        }
        if let Some(reason) = heard.blocked.or(heard.halted) {
            return Err(Error::Denied(format!(
                "a hook blocked the prompt: {reason}"
            )));
        }
        if let Some(note) = context_note(&heard.context) {
            annotate(input, &note);
        }
        Ok(())
    }

    /// Runs the `session_start` hooks now if they are due; their context waits for the next prompt.
    pub async fn begin(&mut self) {
        let start = match &self.pending_hooks.opening {
            Opening::Fresh => self.source(),
            Opening::Due(start) => *start,
            Opening::Held(_) | Opening::Done => return,
        };
        let heard = self.opened(start).await;
        self.pending_hooks.opening = Opening::Held(heard);
    }

    async fn take_opening(&mut self) -> Verdict {
        self.begin().await;
        match std::mem::replace(&mut self.pending_hooks.opening, Opening::Done) {
            Opening::Held(heard) => heard,
            Opening::Fresh | Opening::Due(_) | Opening::Done => Verdict::default(),
        }
    }

    pub(crate) async fn clear(&mut self) -> Result<Vec<String>> {
        let notes = self.end(End::Clear).await;
        self.clear_history()?;
        self.pending_hooks.reopen(Start::Clear);
        self.begin().await;
        Ok(notes)
    }

    pub(in crate::runtime) fn set_subagent(&mut self, agent: &str) {
        let id = self.session_id();
        self.hooks.set_subagent(id, agent.to_string());
    }

    async fn opened(&self, start: Start) -> Verdict {
        if let Some(agent) = self.hooks.subagent() {
            let mut started = self
                .hook(HookEvent::SubagentStart, Some(&agent.name), json!({}))
                .await;
            started.blocked = None;
            started.halted = None;
            return started;
        }
        if self.session.depth() > 0 {
            return Verdict::default();
        }
        let mut started = self.session_start(start).await;
        started.blocked = None;
        started
    }

    pub(in crate::runtime) async fn session_start(&self, start: Start) -> Verdict {
        let payload = json!({ "source": start.name(), "model": self.model });
        self.hook(HookEvent::SessionStart, Some(start.name()), payload)
            .await
    }

    fn source(&self) -> Start {
        if self.session.turns() == 0 {
            Start::Startup
        } else if self.session.parent().is_some() {
            Start::Fork
        } else {
            Start::Resume
        }
    }

    pub(in crate::runtime) async fn stop_hook(
        &self,
        turn: &Turn,
        active: bool,
        emit: &mut impl FnMut(&Event),
    ) -> Option<String> {
        let mut payload = json!({
            "text": turn.text,
            "aborted": turn.aborted,
            "stop_hook_active": active,
        });
        let agent = self.hooks.subagent();
        let event = if agent.is_some() {
            payload["agent_transcript"] = json!(self.session.path());
            HookEvent::SubagentStop
        } else {
            HookEvent::Stop
        };
        let subject = agent.map(|agent| agent.name.as_str());
        let heard = self.hook(event, subject, payload).await;
        for note in heard.notes.iter().chain(&heard.halted) {
            emit(&note_event(event, note));
        }
        let feedback = heard.feedback();
        (heard.halted.is_none() && !feedback.is_empty()).then(|| feedback.join("\n\n"))
    }

    pub(in crate::runtime) async fn stop_failure(&self, failure: &ProviderFailure) {
        if self.hooks.subagent().is_some() {
            return;
        }
        let error = Failure::of(failure);
        let payload = json!({
            "error": error.name(),
            "error_details": failure.status().map(|status| format!("HTTP {status}")),
            "text": failure.message(),
        });
        drop(
            self.hook(HookEvent::StopFailure, Some(error.name()), payload)
                .await,
        );
    }

    /// Runs the `session_end` hooks with reason `other` and returns their notes for the user.
    pub async fn close(&self) -> Vec<String> {
        self.end(End::Other).await
    }

    pub(crate) async fn end(&self, reason: End) -> Vec<String> {
        if self.session.depth() > 0 {
            return Vec::new();
        }
        let payload = json!({ "reason": reason.name() });
        self.hook(HookEvent::SessionEnd, Some(reason.name()), payload)
            .await
            .notes
    }
}
