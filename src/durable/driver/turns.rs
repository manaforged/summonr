use super::*;

impl SessionDriver {
    pub(crate) fn start_input(&mut self, input: &UserInput) -> Result<Vec<String>, DriverError> {
        if input.text.trim().is_empty() && input.images.is_empty() {
            return Err(DriverError::Empty);
        }
        if self.turn.is_some() {
            return Err(DriverError::Busy);
        }
        self.pending.clear();
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        let stage = work.stage().ok_or(DriverError::NoWork)?;
        let queued = if stage == Stage::Execute {
            std::mem::take(&mut work.pending)
        } else {
            Vec::new()
        };
        if !queued.is_empty() {
            self.checkpoint()?;
        }
        for text in &queued {
            self.append(Line::User {
                text: text.clone(),
                source: None,
                images: Vec::new(),
                start: false,
            })?;
        }
        self.append(Line::User {
            text: input.text.clone(),
            source: input.source.as_ref().map(|source| source.text.clone()),
            images: input.images.clone(),
            start: true,
        })?;
        self.turn = Some(Turn {
            stage: Some(stage),
            round: None,
            calls: Vec::new(),
        });
        Ok(queued)
    }

    pub(crate) fn steer(&mut self, input: &UserInput) -> Result<(), DriverError> {
        if input.text.trim().is_empty() && input.images.is_empty() {
            return Err(DriverError::Empty);
        }
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.append(Line::User {
            text: input.text.clone(),
            source: input.source.as_ref().map(|source| source.text.clone()),
            images: input.images.clone(),
            start: false,
        })
    }

    pub(crate) fn start_provider(
        &mut self,
        model: &str,
        exposure: Exposure,
    ) -> Result<Attempt, DriverError> {
        self.start_round(Purpose::Agent, model, exposure)
    }

    pub(crate) fn start_round(
        &mut self,
        purpose: Purpose,
        model: &str,
        exposure: Exposure,
    ) -> Result<Attempt, DriverError> {
        let attempt = Attempt(self.take());
        let turn = self.turn.as_mut().ok_or(DriverError::NoTurn)?;
        if turn.round.is_some() {
            return Err(DriverError::State(
                "a provider round is already open".to_string(),
            ));
        }
        turn.round = Some(Round {
            attempt,
            purpose,
            exposure,
            model: model.to_string(),
        });
        Ok(attempt)
    }

    pub(super) fn end_round(&mut self, attempt: Attempt) -> Result<Round, DriverError> {
        let turn = self.turn.as_mut().ok_or(DriverError::NoTurn)?;
        match turn.round.take() {
            Some(round) if round.attempt == attempt => Ok(round),
            other => {
                turn.round = other;
                Err(DriverError::NoProvider)
            }
        }
    }

    pub(crate) fn finish_provider(
        &mut self,
        attempt: Attempt,
        text: &str,
        reasoning: &str,
        usage: Usage,
        meta: UsageMeta,
    ) -> Result<(), DriverError> {
        let round = self.end_round(attempt)?;
        let charge = |note: &str| Line::Charge {
            reported: true,
            usage,
            note: note.to_string(),
            model: round.model.clone(),
            prefix_hash: meta.prefix_hash.clone(),
            cost: meta.cost,
        };
        let line = match round.purpose {
            Purpose::Agent => return self.answer(round, text, reasoning, usage, &[], meta),
            Purpose::Summary => charge("compaction summary"),
            Purpose::Judge => charge("goal judge"),
            Purpose::Review => charge("review"),
            Purpose::Scout => charge("scout"),
        };
        self.append(line)
    }

    pub(crate) fn finish_provider_with_tools(
        &mut self,
        attempt: Attempt,
        text: &str,
        reasoning: &str,
        usage: Usage,
        tools: &[ToolCall],
        meta: UsageMeta,
    ) -> Result<Vec<Call>, DriverError> {
        let round = self.end_round(attempt)?;
        if round.purpose != Purpose::Agent {
            return Err(DriverError::NoProvider);
        }
        let planned: Vec<(Call, String)> = tools
            .iter()
            .map(|call| (Call(self.take()), call.id.clone()))
            .collect();
        self.answer(round, text, reasoning, usage, tools, meta)?;
        let ids = planned.iter().map(|(call, _)| *call).collect();
        self.fresh &= planned.is_empty();
        if let Some(turn) = self.turn.as_mut() {
            turn.calls = planned;
        }
        Ok(ids)
    }

    pub(super) fn answer(
        &mut self,
        round: Round,
        text: &str,
        reasoning: &str,
        usage: Usage,
        tools: &[ToolCall],
        meta: UsageMeta,
    ) -> Result<(), DriverError> {
        self.append(Line::Assistant {
            text: text.to_string(),
            reasoning: reasoning.to_string(),
            tool_calls: tools.to_vec(),
            usage,
            model: round.model,
            prefix_hash: meta.prefix_hash,
            cost: meta.cost,
            continuation: meta.continuation,
        })
    }

    pub(crate) fn fail_provider(
        &mut self,
        attempt: Attempt,
        diagnostic: &str,
        millis: u64,
        billable: bool,
    ) -> Result<(), DriverError> {
        let reported = self.partial.usage.take();
        if !self.partial.text.is_empty() || !self.partial.reasoning.is_empty() {
            self.append(Line::Interrupted {
                text: self.partial.text.clone(),
                reasoning: self.partial.reasoning.clone(),
            })?;
            self.partial = Partial::default();
        }
        let round = self.end_round(attempt)?;
        let (input, output) = if billable {
            (
                round.exposure.max_input_tokens,
                round.exposure.max_output_tokens,
            )
        } else {
            (0, 0)
        };
        self.append(Line::Charge {
            reported: reported.is_some(),
            usage: reported.map_or(
                Usage {
                    input,
                    output,
                    reasoning: 0,
                    cached: 0,
                    written: 0,
                    millis,
                },
                |usage| Usage::measured(&usage, millis),
            ),
            note: diagnostic.to_string(),
            model: round.model,
            prefix_hash: String::new(),
            cost: None,
        })
    }

    pub(crate) fn finish_tool(
        &mut self,
        planned: Call,
        text: &str,
        success: bool,
        mutated: bool,
        millis: u64,
        patch: Option<String>,
    ) -> Result<(), DriverError> {
        let id = self
            .turn
            .as_ref()
            .and_then(|turn| {
                turn.calls
                    .iter()
                    .find(|(call, _)| *call == planned)
                    .map(|(_, id)| id.clone())
            })
            .ok_or(DriverError::NoTool)?;
        self.append(Line::Tool {
            call: id,
            text: text.to_string(),
            success,
            mutated,
            millis,
            patch,
        })
    }

    pub(crate) fn close(&mut self) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.append(Line::Close {
            reason: None,
            code: None,
            footprint_kb: crate::supervise::footprint_kb(None),
        })?;
        self.turn = None;
        self.fresh = false;
        self.remember_tree();
        Ok(())
    }

    pub(crate) fn abort(
        &mut self,
        abort: &crate::tools::Abort,
        receipt: Option<&crate::verify::Receipt>,
    ) -> Result<(), DriverError> {
        let reason = abort.reason.as_str();
        if self.turn.is_none() && receipt.is_none() {
            return self.interrupt_children(reason, abort.code.child_scope());
        }
        if let Some(attempt) = self
            .turn
            .as_ref()
            .and_then(|turn| turn.round.as_ref())
            .map(|round| round.attempt)
        {
            self.fail_provider(attempt, reason, 0, true)?;
        }
        for call in self.state.unanswered.clone() {
            self.abort_call(call)?;
        }
        self.append(Line::Close {
            reason: Some(reason.to_string()),
            code: Some(abort.code),
            footprint_kb: crate::supervise::footprint_kb(None),
        })?;
        if let Some(receipt) = receipt {
            self.append(Line::Receipt {
                text: String::new(),
                receipt: Some(receipt.clone()),
            })?;
        }
        self.turn = None;
        self.fresh = false;
        self.remember_tree();
        if let Some(work) = &mut self.work
            && work.interrupt()
        {
            self.checkpoint()?;
        }
        self.interrupt_children(reason, abort.code.child_scope())
    }

    fn abort_call(&mut self, call: String) -> Result<(), DriverError> {
        let finished = self.state.children.values().find(|child| {
            child.call.as_deref() == Some(call.as_str())
                && matches!(child.state, crate::child::ChildState::Completed(_))
        });
        let (text, success) = match finished.map(serde_json::to_string) {
            Some(Ok(text)) => (text, true),
            _ => ("aborted".to_string(), false),
        };
        self.append(Line::Tool {
            call,
            text,
            success,
            mutated: false,
            millis: 0,
            patch: None,
        })
    }
}
