use super::record::*;
use super::*;

pub(super) struct Run<P: Provider> {
    pub(super) child: Loop<P>,
    pub(super) record: ChildRecord,
    pub(super) guard: Guard,
    pub(super) assignment: String,
    pub(super) deadline: Option<Duration>,
    pub(super) started: Instant,
    pub(super) state: watch::Sender<ChildState>,
    pub(super) stay: bool,
}

pub(super) struct Guard {
    pub(super) tree: Option<Tree>,
}

impl Guard {
    pub(super) fn disarm(&mut self) -> Option<Tree> {
        self.tree.take()
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(tree) = self.tree.take()
            && tree.unchanged().is_ok_and(|same| same)
        {
            drop(tree.remove_child());
        }
    }
}

impl<P: Provider> Run<P> {
    pub(super) async fn execute(mut self, sender: UnboundedSender<Event>) -> Done {
        let id = self.record.id.to_string();
        let emit = |event: &Event| {
            drop(sender.send(Event::Child {
                id: id.clone(),
                event: Box::new(event.clone()),
            }));
        };
        let outcome = self.drive(emit).await;
        let usage = self.report(outcome, &sender, &id);
        if self.stay {
            self.mail_loop(&sender, &id).await;
        }
        let usage = self.reported_usage(usage);
        let paths = self.child.session.touched();
        Done {
            record: self.record,
            tree: self.guard.disarm(),
            paths,
            usage,
            billed: self.child.session.spent_tokens(),
            settled: false,
            millis: super::elapsed_millis(self.started),
        }
    }

    async fn drive(&mut self, emit: impl FnMut(&Event)) -> Option<Result<Turn>> {
        let Some(deadline) = self.deadline else {
            return Some(self.child.turn_with(self.assignment.as_str(), emit).await);
        };
        let turn = Box::pin(self.child.turn_with(self.assignment.as_str(), emit));
        crate::limits::within(deadline, "the child turn", turn)
            .await
            .ok()
    }

    async fn mail_loop(&mut self, sender: &UnboundedSender<Event>, id: &str) {
        loop {
            let prompt = next_mail(&mut self.child).await;
            self.state.send_replace(ChildState::Running);
            let prompt = self.child.steered(&prompt);
            let emit = |event: &Event| {
                drop(sender.send(Event::Child {
                    id: id.to_string(),
                    event: Box::new(event.clone()),
                }));
            };
            let outcome = self.child.turn_with(prompt.as_str(), emit).await;
            self.report(Some(outcome), sender, id);
        }
    }

    fn report(
        &mut self,
        outcome: Option<Result<Turn>>,
        sender: &UnboundedSender<Event>,
        id: &str,
    ) -> Usage {
        let (state, usage) = self.outcome_state(outcome);
        let event = match &state {
            ChildState::Completed(result) => Some(Event::Note {
                text: result.text.clone(),
            }),
            ChildState::Running
            | ChildState::Aborted(_)
            | ChildState::Failed(_)
            | ChildState::Interrupted(_) => state.notification(),
        };
        self.record.state = state;
        if self.stay {
            self.state.send_replace(self.record.state.clone());
        }
        if let Some(event) = event {
            drop(sender.send(Event::Child {
                id: id.to_string(),
                event: Box::new(event),
            }));
        }
        usage
    }

    fn outcome_state(&mut self, outcome: Option<Result<Turn>>) -> (ChildState, Usage) {
        match outcome {
            Some(Ok(turn)) => {
                let state = if turn.aborted {
                    ChildState::Aborted(self.child.result(
                        &turn.text,
                        turn.rounds,
                        turn.usage.clone(),
                        Some(&turn.text),
                    ))
                } else {
                    ChildState::Completed(self.child.result(
                        &turn.text,
                        turn.rounds,
                        turn.usage.clone(),
                        None,
                    ))
                };
                (state, turn.usage)
            }
            Some(Err(error)) => (
                ChildState::Failed(self.child.result(
                    "",
                    0,
                    Usage::default(),
                    Some(&error.to_string()),
                )),
                Usage::default(),
            ),
            None => (
                ChildState::Aborted(self.deadline_result()),
                Usage::default(),
            ),
        }
    }

    fn deadline_result(&mut self) -> ChildResult {
        let reason = self.deadline.map_or_else(
            || "child deadline reached".to_string(),
            |deadline| format!("child deadline reached after {}s", deadline.as_secs()),
        );
        let diagnostic = match self.child.abort_note(&reason) {
            Ok(()) => reason,
            Err(error) => format!("{reason}; cannot close child turn: {error}"),
        };
        self.child
            .result("", 0, Usage::default(), Some(&diagnostic))
    }

    fn reported_usage(&mut self, mut usage: Usage) -> Usage {
        if matches!(self.record.state, ChildState::Completed(_)) {
            return usage;
        }
        match accounting_at(self.child.session.path()) {
            Ok((_, reported)) => {
                usage = reported;
                if let Some(result) = self.record.state.result_mut() {
                    result.usage.clone_from(&usage);
                }
            }
            Err(error) => self.record.fail_record(&error.to_string()),
        }
        usage
    }
}

pub(crate) async fn next_mail<P: Provider>(child: &mut Loop<P>) -> String {
    let limits = crate::limits::limits();
    let mut wait = limits.mail_poll();
    loop {
        child.mail_heartbeat(crate::mail::Presence::Idle);
        if let Some(mailbox) = child.mailbox.as_mut()
            && let Ok(notes) = mailbox.poll()
        {
            child.inbox.extend(notes);
        }
        let (front, back) = child.inbox.as_slices();
        if child.mail_wakes(front) || child.mail_wakes(back) {
            let text = child
                .inbox
                .drain(..)
                .map(|note| crate::mail::envelope(&note))
                .collect::<Vec<_>>()
                .join("\n\n");
            if !text.trim().is_empty() {
                return text;
            }
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(limits.beat());
    }
}
