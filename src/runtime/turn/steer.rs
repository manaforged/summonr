use super::*;

const CONTINUE: &str = "[summon control: continue the task. Make the next tool call; \
                        reply without one only if the task is complete.]";

impl<P: Provider> Loop<P> {
    pub(super) async fn steer_round(
        &mut self,
        round: usize,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<String>> {
        self.guard.round();
        if let Some(reason) = self.guard.stuck()
            && let Some(reason) = self.steer_stuck(reason, messages, events, emit).await?
        {
            return Ok(Some(reason));
        }
        if self.checkpoint_rounds > 0 && round.is_multiple_of(self.checkpoint_rounds) {
            self.steer_checkpoint(round, messages, events, emit)?;
        }
        Ok(None)
    }

    async fn steer_stuck(
        &mut self,
        reason: String,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<String>> {
        let interactive = self.steer_shared.is_some();
        if interactive && self.guard.steered() {
            let question = format!("{reason}. Let the turn continue?");
            if self.approve(question, false, events, emit).await? == GrantAnswer::Deny {
                return Ok(Some(reason));
            }
            self.guard.forgive();
            return Ok(None);
        }
        self.inject(
            format!(
                "[summon control: {reason}. Before the next tool call, state the \
                 hypothesis the next action tests and what result would falsify it. \
                 If the same outcome repeats, change the approach.]"
            ),
            messages,
        )?;
        self.guard.control();
        self.guard.mark_steered();
        if !interactive {
            self.guard.forgive();
        }
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!("loop guard: {reason} — steering the model"),
            },
        )?;
        Ok(None)
    }

    fn steer_checkpoint(
        &mut self,
        round: usize,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let progress = self.work_progress()?;
        if progress.ledger.is_empty() {
            return Ok(());
        }
        self.inject(
            format!(
                "[summon control: checkpoint — update the ledger before continuing: what is \
                 proven, what is the current blocker, what is next. If the last actions did \
                 not move the work forward, change approach or ask.]\nCurrent ledger:\n{}",
                progress.ledger.join("\n")
            ),
            messages,
        )?;
        self.guard.control();
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!("checkpoint at round {round}"),
            },
        )
    }

    pub(super) fn reprompts(&mut self, reply: &Message) -> bool {
        self.steer_shared.is_none()
            && !self.is_plan()
            && !reply.content.trim().is_empty()
            && self.guard.reprompt()
    }

    pub(super) fn steer_continue(
        &mut self,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.inject(CONTINUE.to_string(), messages)?;
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "a control message got no tool call — asking once to continue".to_string(),
            },
        )
    }

    pub(super) fn inject(&mut self, text: String, messages: &mut Vec<Message>) -> Result<()> {
        let input = UserInput::from(text);
        self.session.steer(&input).map_err(super::session_err)?;
        messages.push(Message::user(input));
        Ok(())
    }
}
