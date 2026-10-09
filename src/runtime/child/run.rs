use super::record::*;
use super::*;

pub(super) struct Run<P: Provider> {
    pub(super) child: Loop<P>,
    pub(super) record: ChildRecord,
    pub(super) guard: Guard,
    pub(super) assignment: String,
    pub(super) deadline: Option<Duration>,
    pub(super) started: Instant,
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
        let access = self.record.access;
        let (state, usage) = self.outcome_state(outcome, access);
        self.record.state = state;
        if let Some(reason) = unfinished_reason(&self.record.state) {
            let code = match self.record.state {
                ChildState::Interrupted(_) => crate::tools::AbortCode::Interrupted,
                ChildState::Running
                | ChildState::Completed(_)
                | ChildState::Aborted(_)
                | ChildState::Failed(_) => crate::tools::AbortCode::Error,
            };
            emit(&Event::Aborted { reason, code });
        }
        let usage = self.reported_usage(usage);
        let paths = match access {
            Access::Implement => Loop::<P>::touched_at(self.child.session.path()),
            Access::Research => Vec::new(),
        };
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

    async fn drive(
        &mut self,
        emit: impl FnMut(&Event),
    ) -> std::result::Result<Result<Turn>, tokio::time::error::Elapsed> {
        match self.deadline {
            Some(deadline) => {
                tokio::time::timeout(
                    deadline,
                    Box::pin(self.child.turn_with(self.assignment.as_str(), emit)),
                )
                .await
            }
            None => Ok(self.child.turn_with(self.assignment.as_str(), emit).await),
        }
    }

    fn outcome_state(
        &mut self,
        outcome: std::result::Result<Result<Turn>, tokio::time::error::Elapsed>,
        access: Access,
    ) -> (ChildState, Usage) {
        match outcome {
            Ok(Ok(turn)) => {
                let state = if turn.aborted {
                    ChildState::Aborted(self.child.result(
                        &turn.text,
                        turn.rounds,
                        turn.usage.clone(),
                        Some(&turn.text),
                        access,
                    ))
                } else {
                    ChildState::Completed(self.child.result(
                        &turn.text,
                        turn.rounds,
                        turn.usage.clone(),
                        None,
                        access,
                    ))
                };
                (state, turn.usage)
            }
            Ok(Err(error)) => {
                let diagnostic = match self.child.abort_note(&error.to_string()) {
                    Ok(()) => error.to_string(),
                    Err(abort) => format!("{error}; cannot close child turn: {abort}"),
                };
                (
                    ChildState::Failed(self.child.result(
                        "",
                        0,
                        Usage::default(),
                        Some(&diagnostic),
                        access,
                    )),
                    Usage::default(),
                )
            }
            Err(_) => (
                ChildState::Aborted(self.deadline_result(access)),
                Usage::default(),
            ),
        }
    }

    fn deadline_result(&mut self, access: Access) -> ChildResult {
        let reason = self.deadline.map_or_else(
            || "child deadline reached".to_string(),
            |deadline| format!("child deadline reached after {}s", deadline.as_secs()),
        );
        let diagnostic = match self.child.abort_note(&reason) {
            Ok(()) => reason,
            Err(error) => format!("{reason}; cannot close child turn: {error}"),
        };
        self.child
            .result("", 0, Usage::default(), Some(&diagnostic), access)
    }

    fn reported_usage(&mut self, mut usage: Usage) -> Usage {
        if matches!(self.record.state, ChildState::Completed(_)) {
            return usage;
        }
        match accounting_at(self.child.session.path()) {
            Ok((_, reported)) => {
                usage = reported;
                if let Some(result) = result_mut(&mut self.record.state) {
                    result.usage.clone_from(&usage);
                }
            }
            Err(error) => self.record.fail_record(&error.to_string()),
        }
        usage
    }
}

fn unfinished_reason(state: &ChildState) -> Option<String> {
    let (label, result) = match state {
        ChildState::Running | ChildState::Completed(_) => return None,
        ChildState::Aborted(result) => ("aborted", result),
        ChildState::Failed(result) => ("failed", result),
        ChildState::Interrupted(result) => ("interrupted", result),
    };
    Some(
        result
            .diagnostic
            .clone()
            .unwrap_or_else(|| label.to_string()),
    )
}
