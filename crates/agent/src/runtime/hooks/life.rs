use super::*;
use crate::hooks::{End, Start};

impl<P: Provider> Loop<P> {
    pub(in crate::runtime) async fn admit(
        &mut self,
        input: &mut UserInput,
        typed: bool,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let mut heard = Verdict::default();
        if self.turn_started == 0 && self.session.depth() == 0 {
            self.turn_started = summonr_core::clock::now();
            let start = self.start();
            let payload = json!({ "source": start.name(), "model": self.model });
            let mut started = self
                .hook(HookEvent::SessionStart, Some(start.name()), payload)
                .await;
            started.blocked = None;
            heard.merge(started);
        }
        if typed {
            let prompt = input
                .source
                .as_ref()
                .map_or(&input.text, |source| &source.text)
                .clone();
            let payload = json!({ "prompt": prompt });
            heard.merge(self.hook(HookEvent::Prompt, None, payload).await);
        }
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

    fn start(&self) -> Start {
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
        let payload = json!({
            "text": turn.text,
            "aborted": turn.aborted,
            "stop_hook_active": active,
        });
        let heard = self.hook(HookEvent::Stop, None, payload).await;
        for note in heard.notes.iter().chain(&heard.halted) {
            emit(&note_event(HookEvent::Stop, note));
        }
        let feedback = heard.feedback();
        (heard.halted.is_none() && !feedback.is_empty()).then(|| feedback.join("\n\n"))
    }

    /// Runs the `session_end` hooks for `reason` and returns their notes for the user.
    pub async fn end(&self, reason: End) -> Vec<String> {
        if self.session.depth() > 0 {
            return Vec::new();
        }
        let payload = json!({ "reason": reason.name() });
        self.hook(HookEvent::SessionEnd, Some(reason.name()), payload)
            .await
            .notes
    }
}
