//! Durable Work lifecycle transitions.

use super::*;

struct PlanReply<'a> {
    attempt: super::id::ProviderAttemptId,
    result: super::id::ProviderResultId,
    reasoning: &'a str,
    usage: Usage,
}

impl SessionDriver {
    pub(crate) fn work_progress(&self) -> Result<crate::WorkProgress, DriverError> {
        let work = self.work_state()?;
        let kind = match work.policy() {
            WorkPolicy::Single => crate::WorkKind::Single,
            WorkPolicy::Steps => crate::WorkKind::Steps,
            WorkPolicy::Goal { .. } => crate::WorkKind::Goal,
        };
        let (phase, reason) = match work.status() {
            WorkStatus::Active(WorkStage::Plan)
            | WorkStatus::Paused {
                stage: WorkStage::Plan,
                ..
            } => (crate::WorkPhase::Plan, None),
            WorkStatus::Active(WorkStage::Execute)
            | WorkStatus::Paused {
                stage: WorkStage::Execute,
                ..
            } => (crate::WorkPhase::Execute, None),
            WorkStatus::Active(WorkStage::Verify)
            | WorkStatus::Paused {
                stage: WorkStage::Verify,
                ..
            } => (crate::WorkPhase::Verify, None),
            WorkStatus::Completed { .. } => (crate::WorkPhase::Completed, None),
            WorkStatus::Abandoned { reason } => (
                crate::WorkPhase::Failed,
                Some(match reason {
                    CloseReason::User => "Work was stopped by the user.".to_string(),
                    CloseReason::Superseded => "Work was superseded.".to_string(),
                    CloseReason::UnrecoverableEffect => {
                        "Work stopped after an unrecoverable tool effect.".to_string()
                    }
                    CloseReason::Other(text) => text.as_str().to_string(),
                }),
            ),
        };
        let current = work
            .active_item
            .and_then(|id| work.items.iter().position(|item| item.id == id))
            .or_else(|| {
                work.items.iter().rposition(|item| {
                    matches!(
                        item.disposition,
                        ItemDisposition::Blocked | ItemDisposition::Completed
                    )
                })
            });
        let item = current.and_then(|index| work.items.as_slice().get(index));
        Ok(crate::WorkProgress {
            kind,
            phase,
            current: (kind == crate::WorkKind::Steps)
                .then(|| current.map(|index| index + 1))
                .flatten(),
            total: work.items.len(),
            attempts: item.map_or(0, |item| item.attempts),
            rounds: item.map_or(0, |item| item.rounds),
            reason,
        })
    }

    pub(crate) fn abandon_work(&mut self, reason: &str) -> Result<(), DriverError> {
        let mut state = self.work_state()?;
        if state.status().terminal() {
            return Ok(());
        }
        let session = self.session();
        let epoch = ContextEpoch {
            id: super::id::ContextEpochId::new(session, self.take()),
            head: self.request_head()?.clone(),
            stage: None,
            base: Items::new(self.current_entries())?,
        };
        let mut items = state.items.as_slice().to_vec();
        if let Some(active) = state.active_item
            && let Some(item) = items.iter_mut().find(|item| item.id == active)
        {
            item.disposition = ItemDisposition::Blocked;
        }
        state.revision = WorkRevision::new(state.revision().get().saturating_add(1))?;
        state.status = WorkStatus::Abandoned {
            reason: CloseReason::Other(Text::new(reason)?),
        };
        state.items = Items::new(items)?;
        state.active_item = None;
        state.projection_owner = WorkProjection::Epoch(epoch.id);
        state.validate()?;
        let work_fact = super::id::FactId::new(session, self.take());
        let epoch_fact = super::id::FactId::new(session, self.take());
        self.append(
            None,
            vec![
                Fact {
                    id: work_fact,
                    data: FactData::WorkSet {
                        work: Box::new(state),
                    },
                },
                Fact {
                    id: epoch_fact,
                    data: FactData::ContextEpochOpened { epoch },
                },
            ],
        )
    }

    pub(crate) fn goal_objective(&self) -> Result<Option<String>, DriverError> {
        let work = self.work_state()?;
        Ok(matches!(work.policy(), WorkPolicy::Goal { .. })
            .then(|| work.objective.as_str().to_string()))
    }

    /// Checks owned by the active Work item, in their declared order.
    pub(crate) fn completion_checks(&self) -> Result<Vec<AcceptanceCheck>, DriverError> {
        let work = self.work_state()?;
        let plan = work
            .selected_plan()
            .and_then(|id| self.writer.state().plans.get(&id))
            .ok_or(DriverError::NoWork)?;
        if work.policy() != WorkPolicy::Steps {
            return Ok(plan.checks.as_slice().to_vec());
        }
        let active = work.active_item.ok_or(DriverError::NoWork)?;
        let step = plan
            .steps
            .iter()
            .find(|step| step.id == active)
            .ok_or(DriverError::NoWork)?;
        Ok(step
            .checks
            .iter()
            .filter_map(|id| plan.checks.iter().find(|check| check.id == *id).cloned())
            .collect())
    }

    /// Capture the Execute result, advance Work to Verify, and close Execute.
    pub(crate) fn enter_verify(&mut self) -> Result<WorkCapture, DriverError> {
        let session = self.session();
        let operation = self.open_operation()?;
        let provider = self
            .latest_provider(operation)
            .ok_or(DriverError::NoProvider)?;
        let mut state = self.work_state()?;
        if state.status().stage() != Some(WorkStage::Execute) {
            return Err(DriverError::NoWork);
        }
        let base_id = state.base().ok_or(DriverError::NoWork)?;
        let base = self
            .writer
            .state()
            .observations
            .get(&base_id)
            .cloned()
            .ok_or(DriverError::NoWork)?;
        let capture = self.observe_workspace()?;
        let manifest = base.changes(&capture)?;
        let paths = manifest.paths().map(path_text).collect::<Vec<_>>();
        let mut candidate = Candidate {
            id: super::id::CandidateId::new(session, self.take()),
            work: state.id(),
            execute_revision: state.revision(),
            plan: state.selected_plan().ok_or(DriverError::NoWork)?,
            policy: state
                .verification()
                .map(VerificationPolicy::id)
                .ok_or(DriverError::NoWork)?,
            scope: state.scope().ok_or(DriverError::NoWork)?,
            base: base_id,
            capture: capture.id(),
            manifest_digest: manifest.digest(),
            digest: Digest::default(),
        };
        candidate.digest = candidate.calculated_digest(base.digest(), capture.digest())?;
        let observation_fact = super::id::FactId::new(session, self.take());
        let evidence = EvidenceRef {
            id: super::id::EvidenceId::new(session, self.take()),
            source: observation_fact,
            digest: candidate.digest,
            summary: Text::new("Workspace candidate captured.")?,
        };
        let mut evidence_items = state.evidence.as_slice().to_vec();
        evidence_items.push(evidence);
        let head = self.request_head()?.clone();
        let epoch = ContextEpoch {
            id: super::id::ContextEpochId::new(session, self.take()),
            head,
            stage: Some(WorkStage::Verify),
            base: Items::new(self.current_entries())?,
        };
        state.revision = WorkRevision::new(state.revision().get().saturating_add(1))?;
        state.status = WorkStatus::Active(WorkStage::Verify);
        state.candidate = Some(candidate.clone());
        state.evidence = Items::new(evidence_items)?;
        state.projection_owner = WorkProjection::Operation(operation);
        state.validate()?;
        let close = OperationClose {
            id: operation,
            outcome: OperationOutcome::Completed,
            provider: Some(provider),
            work: Some(state.id()),
            work_status: Some(state.status().clone()),
            plan: state.selected_plan(),
            candidate: Some(candidate.id()),
            verdict: None,
            usage_digest: Digest::hash(b"mage:usage-digest:v1", b"execute"),
            inputs: Items::default(),
            diagnostic: None,
        };
        let work_fact = super::id::FactId::new(session, self.take());
        let epoch_fact = super::id::FactId::new(session, self.take());
        let close_fact = super::id::FactId::new(session, self.take());
        self.append(
            None,
            vec![
                Fact {
                    id: observation_fact,
                    data: FactData::WorkspaceObserved {
                        observation: capture,
                    },
                },
                Fact {
                    id: work_fact,
                    data: FactData::WorkSet {
                        work: Box::new(state),
                    },
                },
                Fact {
                    id: epoch_fact,
                    data: FactData::ContextEpochOpened { epoch },
                },
                Fact {
                    id: close_fact,
                    data: FactData::OperationClosed {
                        close,
                        stage_outcome: OperationStageOutcome::Advanced,
                    },
                },
            ],
        )?;
        Ok(WorkCapture { paths, provider })
    }

    /// Capture the final candidate observation before its atomic Work verdict.
    pub(crate) fn prepare_verdict(
        &mut self,
        mut evidence: Vec<EvidenceRef>,
        mut passed: bool,
    ) -> Result<PreparedVerdict, DriverError> {
        let session = self.session();
        let operation = self.open_operation()?;
        let state = self.work_state()?;
        let candidate = state.candidate().ok_or(DriverError::NoWork)?;
        let capture = self
            .writer
            .state()
            .observations
            .get(&candidate.capture)
            .cloned()
            .ok_or(DriverError::NoWork)?;
        let observation = self.observe_workspace()?;
        let mut diagnostic = None;
        let mut synthetic = None;
        if observation.digest() != capture.digest() {
            let output = "workspace changed after candidate capture";
            let source = super::id::FactId::new(session, self.take());
            let finding = EvidenceRef {
                id: super::id::EvidenceId::new(session, self.take()),
                source,
                digest: completion_digest("workspace candidate unchanged", false, output),
                summary: Text::new("Workspace changed during verification.")?,
            };
            evidence.push(finding);
            synthetic = Some(Fact {
                id: source,
                data: FactData::CompletionCheckRecorded {
                    operation,
                    check: CompletionCheck::CandidateStable,
                    command: Text::new("workspace candidate unchanged")?,
                    passed: false,
                    output: Text::new(output)?,
                },
            });
            diagnostic = Some(output.to_string());
            passed = false;
        }
        Ok(PreparedVerdict {
            passed,
            diagnostic,
            observation,
            evidence,
            synthetic,
        })
    }

    /// Commit the prepared Verify verdict, visible receipt, and operation close
    /// in one journal append.
    pub(crate) fn commit_verdict(
        &mut self,
        prepared: PreparedVerdict,
        decision: VerdictDecision,
        receipt: Option<(&str, super::id::ProviderResultId)>,
    ) -> Result<(), DriverError> {
        let session = self.session();
        let operation = self.open_operation()?;
        let mut state = self.work_state()?;
        if state.status().stage() != Some(WorkStage::Verify) {
            return Err(DriverError::NoWork);
        }
        let mut evidence = state.evidence.as_slice().to_vec();
        for finding in &prepared.evidence {
            if !evidence.iter().any(|known| known.id == finding.id) {
                evidence.push(finding.clone());
            }
        }
        let verdict = prepared
            .evidence
            .last()
            .or_else(|| evidence.last())
            .map(|finding| finding.id)
            .ok_or(DriverError::NoWork)?;
        let mut items = state.items.as_slice().to_vec();
        let evidence_ids = Items::new(evidence.iter().map(|finding| finding.id).collect())?;
        let active = state
            .active_item
            .and_then(|id| items.iter().position(|item| item.id == id))
            .ok_or(DriverError::NoWork)?;
        items[active].evidence = evidence_ids;
        items[active].rounds = items[active].rounds.saturating_add(1);
        let repair_limit = state
            .verification()
            .map(|policy| policy.repair_limit)
            .unwrap_or(3);
        let mut continuation = None;
        if prepared.passed {
            items[active].attempts = 0;
            match state.policy() {
                WorkPolicy::Single => {
                    items[active].disposition = ItemDisposition::Completed;
                    state.active_item = None;
                    let candidate = state
                        .candidate()
                        .map(Candidate::id)
                        .ok_or(DriverError::NoWork)?;
                    state.status = WorkStatus::Completed { candidate, verdict };
                }
                WorkPolicy::Steps => {
                    items[active].disposition = ItemDisposition::Completed;
                    if let Some(next) = items
                        .iter_mut()
                        .skip(active.saturating_add(1))
                        .find(|item| item.disposition == ItemDisposition::Pending)
                    {
                        next.disposition = ItemDisposition::Active;
                        state.active_item = Some(next.id);
                        state.status = WorkStatus::Active(WorkStage::Execute);
                    } else {
                        state.active_item = None;
                        let candidate = state
                            .candidate()
                            .map(Candidate::id)
                            .ok_or(DriverError::NoWork)?;
                        state.status = WorkStatus::Completed { candidate, verdict };
                    }
                }
                WorkPolicy::Goal { max_turns } => match decision {
                    VerdictDecision::GoalComplete(_reason) => {
                        items[active].disposition = ItemDisposition::Completed;
                        state.active_item = None;
                        let candidate = state
                            .candidate()
                            .map(Candidate::id)
                            .ok_or(DriverError::NoWork)?;
                        state.status = WorkStatus::Completed { candidate, verdict };
                    }
                    VerdictDecision::GoalContinue(reason) if items[active].rounds < max_turns => {
                        items[active].disposition = ItemDisposition::Active;
                        state.status = WorkStatus::Active(WorkStage::Execute);
                        continuation = Some(format!(
                            "[summon goal: the objective is not complete — {reason}. Continue with the next concrete action.]"
                        ));
                    }
                    VerdictDecision::GoalContinue(_) => {
                        items[active].disposition = ItemDisposition::Blocked;
                        state.active_item = None;
                        state.status = WorkStatus::Abandoned {
                            reason: CloseReason::Other(Text::new(format!(
                                "goal reached its {max_turns} turn limit"
                            ))?),
                        };
                    }
                    VerdictDecision::Automatic => {
                        return Err(DriverError::Import(
                            "Goal Work requires a completion decision".to_string(),
                        ));
                    }
                },
            }
        } else {
            items[active].attempts = items[active].attempts.saturating_add(1);
            if state.policy() != WorkPolicy::Single && items[active].attempts >= repair_limit {
                items[active].disposition = ItemDisposition::Blocked;
                state.active_item = None;
                state.status = WorkStatus::Abandoned {
                    reason: CloseReason::Other(Text::new("completion checks failed three times")?),
                };
            } else {
                state.candidate = None;
                items[active].disposition = ItemDisposition::Active;
                state.status = WorkStatus::Active(WorkStage::Execute);
                continuation = Some("Repair the failed completion check.".to_string());
            }
        }
        state.revision = WorkRevision::new(state.revision().get().saturating_add(1))?;
        state.items = Items::new(items)?;
        state.evidence = Items::new(evidence)?;
        let mut facts = vec![Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::WorkspaceObserved {
                observation: prepared.observation,
            },
        }];
        if let Some(fact) = prepared.synthetic {
            facts.push(fact);
        }
        let (outcome, stage_outcome, next_stage) = if state.status().terminal() {
            state.projection_owner = WorkProjection::Operation(operation);
            (
                if prepared.passed {
                    OperationOutcome::Completed
                } else {
                    OperationOutcome::Failed
                },
                OperationStageOutcome::Terminal,
                None,
            )
        } else if prepared.passed {
            state.projection_owner = WorkProjection::Operation(operation);
            (
                OperationOutcome::Completed,
                OperationStageOutcome::Returned,
                Some(WorkStage::Execute),
            )
        } else {
            let finding = prepared.evidence.last().ok_or(DriverError::NoWork)?;
            state.projection_owner = WorkProjection::CompletionCheck(finding.source);
            (
                OperationOutcome::Failed,
                OperationStageOutcome::Returned,
                Some(WorkStage::Execute),
            )
        };
        state.validate()?;
        let close = OperationClose {
            id: operation,
            outcome,
            provider: self.latest_provider(operation),
            work: Some(state.id()),
            work_status: Some(state.status().clone()),
            plan: state.selected_plan(),
            candidate: state.candidate().map(Candidate::id),
            verdict: Some(verdict),
            usage_digest: Digest::hash(b"mage:usage-digest:v1", b"verify"),
            inputs: Items::default(),
            diagnostic: prepared.diagnostic.as_deref().map(Text::new).transpose()?,
        };
        facts.push(Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::WorkSet {
                work: Box::new(state.clone()),
            },
        });
        facts.push(Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::OperationClosed {
                close,
                stage_outcome,
            },
        });
        let epoch = ContextEpoch {
            id: super::id::ContextEpochId::new(session, self.take()),
            head: self.request_head()?.clone(),
            stage: next_stage,
            base: Items::new(self.current_entries())?,
        };
        let epoch_id = epoch.id;
        facts.push(Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::ContextEpochOpened { epoch },
        });
        if let Some((text, provider)) = receipt {
            let mut entry = ContextEntry {
                id: super::id::ContextEntryId::new(session, self.take()),
                epoch: epoch_id,
                role: ContextRole::Assistant,
                source: ContextSource::Receipt(provider),
                payload: Text::new(text)?,
                digest: Digest::default(),
            };
            entry.digest = entry.calculated_digest();
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            });
        }
        if let Some(text) = continuation {
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::InputAccepted {
                    input: super::id::InputId::new(session, self.take()),
                    origin: InputOrigin::WorkPolicy,
                    route: InputRoute::Queue,
                    payload: UserPayload {
                        text: Text::new(&text)?,
                        images: Items::default(),
                    },
                    reserved_projection_bytes: 128,
                },
            });
        }
        self.append(None, facts)
    }

    /// Set the session Work in the Plan stage.
    pub(crate) fn set_work(&mut self, objective: &str) -> Result<super::id::WorkId, DriverError> {
        self.set_work_with(objective, WorkPolicy::Steps, 1)
    }

    /// Open or reuse the Single Work that owns an interactive Plan draft.
    pub(crate) fn prepare_plan_work(
        &mut self,
        objective: &str,
        commands: &[String],
    ) -> Result<(), DriverError> {
        let active = self
            .writer
            .state()
            .works
            .values()
            .find(|work| !work.status().terminal());
        match active {
            None => {
                self.set_work_with(objective, WorkPolicy::Single, commands.len())?;
                Ok(())
            }
            Some(work)
                if work.policy() == WorkPolicy::Single
                    && work.status().stage() == Some(WorkStage::Plan) =>
            {
                Ok(())
            }
            Some(_) => Err(DriverError::Import(
                "another Work is already active".to_string(),
            )),
        }
    }

    fn set_work_with(
        &mut self,
        objective: &str,
        policy: WorkPolicy,
        checks: usize,
    ) -> Result<super::id::WorkId, DriverError> {
        let session = self.session();
        let work = super::id::WorkId::new(session, self.take());
        let epoch = ContextEpoch {
            id: super::id::ContextEpochId::new(session, self.take()),
            head: self.request_head()?.clone(),
            stage: Some(WorkStage::Plan),
            base: Items::new(self.current_entries())?,
        };
        let items = if policy == WorkPolicy::Steps {
            Items::default()
        } else {
            Items::new(vec![WorkItem {
                id: super::id::WorkItemId::new(session, self.take()),
                disposition: ItemDisposition::Pending,
                title: Text::new("Objective")?,
                evidence: Items::default(),
                attempts: 0,
                rounds: 0,
            }])?
        };
        let entry_id = super::id::ContextEntryId::new(session, self.take());
        let mut entry = ContextEntry {
            id: entry_id,
            epoch: epoch.id,
            role: ContextRole::System,
            source: ContextSource::Work(work),
            payload: Text::new(objective)?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        let acceptance_checks = (0..checks)
            .map(|_| super::id::CheckId::new(session, self.take()))
            .collect::<Vec<_>>();
        let state = WorkState {
            id: work,
            revision: WorkRevision::new(1)?,
            objective: Text::new(objective)?,
            status: WorkStatus::Active(WorkStage::Plan),
            policy,
            selected_plan: None,
            items,
            active_item: None,
            constraints: Items::new(vec![Text::new("One state owner")?])?,
            acceptance_checks: Items::new(acceptance_checks)?,
            scope: None,
            base: None,
            candidate: None,
            verification: None,
            evidence: Items::default(),
            predecessor: None,
            projection: Text::new("Work projection")?,
            projection_owner: WorkProjection::Context(entry_id),
        };
        state.validate()?;
        let work_fact = super::id::FactId::new(session, self.take());
        let facts = vec![
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEpochOpened { epoch },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            },
            Fact {
                id: work_fact,
                data: FactData::WorkSet {
                    work: Box::new(state),
                },
            },
        ];
        let command = self.accepted(b"mage:command:work", objective.as_bytes(), work_fact);
        self.append(Some(command), facts)?;
        Ok(work)
    }

    /// Select a supplied Plan revision for the current Work.
    pub(crate) fn select_plan(
        &mut self,
        markdown: &str,
    ) -> Result<super::id::PlanRevisionId, DriverError> {
        self.select_plan_with(markdown, &["cargo check".to_string()], None)
    }

    /// Finish a Plan provider round and select its generated Plan atomically.
    pub(crate) fn finish_plan(
        &mut self,
        provider: super::id::ProviderAttemptId,
        markdown: &str,
        reasoning: &str,
        usage: Usage,
        commands: &[String],
    ) -> Result<super::id::PlanRevisionId, DriverError> {
        let result = self.open_provider_result()?;
        self.select_plan_with(
            markdown,
            commands,
            Some(PlanReply {
                attempt: provider,
                result,
                reasoning,
                usage,
            }),
        )
    }

    fn select_plan_with(
        &mut self,
        markdown: &str,
        commands: &[String],
        reply: Option<PlanReply<'_>>,
    ) -> Result<super::id::PlanRevisionId, DriverError> {
        let workspace = self.workspace.clone().ok_or(DriverError::NoWorkspace)?;
        let platform = self.scope()?.platform();
        let session = self.session();
        let work = self.work_id()?;
        let revision = super::id::PlanRevisionId::new(session, self.take());
        let attempt = super::id::MaterializationId::new(session, self.take());
        let mut state = self.work_state()?;
        let check_specs = plan_checks(markdown, state.policy(), commands)?;
        let check_ids = state.acceptance_checks.as_slice().to_vec();
        if check_ids.len() != check_specs.len() {
            return Err(DriverError::Import(
                "completion checks changed while Work was in Plan".to_string(),
            ));
        }
        let checks = check_ids
            .iter()
            .copied()
            .zip(&check_specs)
            .map(|(id, (command, observation))| {
                Ok(AcceptanceCheck {
                    id,
                    command: Text::new(command.as_str())?,
                    required_observation: observation.clone(),
                })
            })
            .collect::<Result<Vec<_>, BoundError>>()?;
        let (items, steps) =
            if state.policy() == WorkPolicy::Steps {
                let parsed = crate::steps::parse(markdown).map_err(|error| {
                    DriverError::Import(format!("numbered Work plan is invalid: {error}"))
                })?;
                let mut items = Vec::with_capacity(parsed.len());
                let mut plan_steps = Vec::with_capacity(parsed.len());
                for (index, step) in parsed.iter().enumerate() {
                    let id = super::id::WorkItemId::new(session, self.take());
                    let mut step_checks = Vec::new();
                    for command in commands.iter().chain(&step.checks) {
                        if let Some((position, _)) = check_specs.iter().enumerate().find(
                            |(_position, (known, observation))| {
                                observation.is_none() && known == command
                            },
                        ) {
                            let check = check_ids[position];
                            if !step_checks.contains(&check) {
                                step_checks.push(check);
                            }
                        }
                    }
                    if index + 1 == parsed.len() {
                        step_checks = check_ids.clone();
                    }
                    items.push(WorkItem {
                        id,
                        disposition: ItemDisposition::Pending,
                        title: Text::new(&step.text)?,
                        evidence: Items::default(),
                        attempts: 0,
                        rounds: 0,
                    });
                    plan_steps.push(PlanStep {
                        id,
                        title: Text::new(&step.text)?,
                        detail: Text::new(&step.text)?,
                        dependencies: Items::default(),
                        checks: Items::new(step_checks)?,
                    });
                }
                (Some(Items::new(items)?), Items::new(plan_steps)?)
            } else {
                (None, Items::default())
            };
        let source = reply.as_ref().map_or_else(
            || PlanSource::Imported {
                source: Digest::hash(b"mage:plan-source:v1", markdown.as_bytes()),
                converter: 1,
            },
            |reply| PlanSource::Generated {
                provider: reply.result,
            },
        );
        let mut plan = PlanRevision {
            format: 1,
            id: revision,
            work,
            parent: None,
            source,
            objective: Text::new(markdown)?,
            constraints: Items::default(),
            steps,
            checks: Items::new(checks)?,
            risks: Items::default(),
            assumptions: Items::default(),
            non_goals: Items::default(),
            markdown: Text::new(markdown)?,
            digest: Digest::default(),
        };
        plan.digest = plan.calculated_digest()?;
        let plan_digest = plan.digest();
        state.revision = WorkRevision::new(state.revision().get().saturating_add(1))?;
        state.selected_plan = Some(revision);
        if let Some(items) = items {
            state.items = items;
            state.active_item = None;
        }
        state.projection_owner = WorkProjection::Plan(revision);
        state.validate()?;
        let footprint = super::publish::footprint_for(platform, session, work, revision, attempt)
            .map_err(|error| DriverError::Work(WorkError::Encoding(error)))?;
        let plan_fact = super::id::FactId::new(session, self.take());
        let mut facts = vec![
            Fact {
                id: plan_fact,
                data: FactData::PlanSet { plan: plan.clone() },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::PlanMaterializationPlanned {
                    attempt,
                    plan: revision,
                    digest: plan_digest,
                    footprint,
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::WorkSet {
                    work: Box::new(state.clone()),
                },
            },
        ];
        if let Some(reply) = reply {
            let outcome = ProviderOutcome::Completed {
                message: ProviderMessage {
                    text: Text::new(markdown)?,
                    reasoning: Text::new(reply.reasoning)?,
                    tool_calls: Items::default(),
                },
                stop_reason: StopReason::Complete,
                reported_usage: reply.usage.clone(),
            };
            let operation = self.open_operation()?;
            let mut terminal = self.provider_finish_facts(
                reply.attempt,
                reply.result,
                &[],
                markdown,
                outcome,
                reply.usage,
            )?;
            terminal.append(&mut facts);
            terminal.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::OperationClosed {
                    close: OperationClose {
                        id: operation,
                        outcome: OperationOutcome::Completed,
                        provider: Some(reply.result),
                        work: Some(state.id()),
                        work_status: Some(state.status().clone()),
                        plan: state.selected_plan(),
                        candidate: None,
                        verdict: None,
                        usage_digest: Digest::hash(b"mage:usage-digest:v1", b"plan"),
                        inputs: Items::default(),
                        diagnostic: None,
                    },
                    stage_outcome: OperationStageOutcome::Stayed,
                },
            });
            self.append(None, terminal)?;
        } else {
            let command = self.accepted(b"mage:command:plan", markdown.as_bytes(), plan_fact);
            self.append(Some(command), facts)?;
        }
        let outcome = super::publish::materialize(&workspace, platform, session, &plan, attempt);
        let published = outcome.usable();
        let epoch = self.epoch()?;
        let entry_id = super::id::ContextEntryId::new(session, self.take());
        let mut entry = ContextEntry {
            id: entry_id,
            epoch,
            role: ContextRole::System,
            source: ContextSource::Plan(revision),
            payload: Text::new(markdown)?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        let mut state = self.work_state()?;
        state.revision = WorkRevision::new(state.revision().get().saturating_add(1))?;
        state.projection_owner = WorkProjection::Context(entry_id);
        state.validate()?;
        let facts = vec![
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::PlanMaterializationFinished {
                    attempt,
                    plan: revision,
                    outcome,
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::WorkSet {
                    work: Box::new(state),
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            },
        ];
        self.append(None, facts)?;
        if !published {
            return Err(DriverError::Import(
                "plan file was not published".to_string(),
            ));
        }
        Ok(revision)
    }

    /// Enter the selected interactive Plan into Execute.
    pub(crate) fn accept_plan(&mut self) -> Result<(), DriverError> {
        let work = self.work_state()?;
        if work.policy() != WorkPolicy::Single
            || work.status().stage() != Some(WorkStage::Plan)
            || work.selected_plan().is_none()
        {
            return Err(DriverError::Import(
                "there is no selected interactive Plan to accept".to_string(),
            ));
        }
        self.enter_execute()
    }

    /// Open or resume Work owned by the Steps or Goal runner.
    pub(crate) fn prepare_external_work(
        &mut self,
        objective: &str,
        markdown: &str,
        policy: WorkPolicy,
        commands: &[String],
    ) -> Result<(), DriverError> {
        if policy == WorkPolicy::Single {
            return Err(DriverError::Import(
                "external Work requires Steps or Goal policy".to_string(),
            ));
        }
        let active = self
            .writer
            .state()
            .works
            .values()
            .find(|work| !work.status().terminal())
            .cloned();
        let mut work = match active {
            Some(work) if work.policy() != policy => {
                return Err(DriverError::Import(
                    "another Work policy is already active".to_string(),
                ));
            }
            Some(work) if work.objective.as_str() != objective => {
                return Err(DriverError::Import(
                    "another Work objective is already active".to_string(),
                ));
            }
            Some(work) => work,
            None => {
                let checks = plan_checks(markdown, policy, commands)?.len();
                self.set_work_with(objective, policy, checks)?;
                self.work_state()?
            }
        };
        if work.selected_plan().is_none() {
            self.select_plan_with(markdown, commands, None)?;
            work = self.work_state()?;
        }
        let plan = work
            .selected_plan()
            .and_then(|id| self.writer.state().plans.get(&id))
            .ok_or(DriverError::NoWork)?;
        let expected = plan_checks(markdown, policy, commands)?;
        let checks_match = plan.markdown() == markdown
            && plan.checks.len() == expected.len()
            && plan
                .checks
                .iter()
                .zip(&expected)
                .all(|(actual, (command, observation))| {
                    actual.command.as_str() == command
                        && actual.required_observation.as_ref() == observation.as_ref()
                });
        if !checks_match {
            return Err(DriverError::Import(
                "the plan or completion checks changed while Work was active".to_string(),
            ));
        }
        match work.status().stage() {
            Some(WorkStage::Plan) => self.enter_execute(),
            Some(WorkStage::Execute) => Ok(()),
            Some(WorkStage::Verify) => Err(DriverError::Import(
                "Work is still in Verify after an interrupted turn".to_string(),
            )),
            None => Err(DriverError::NoWork),
        }
    }

    /// Attach one provider turn to the active Work, or create a Single Work
    /// when ordinary mode supplies an objective.
    pub(crate) fn prepare_turn_work(
        &mut self,
        objective: Option<&str>,
        commands: &[String],
    ) -> Result<(), DriverError> {
        let active = self
            .writer
            .state()
            .works
            .values()
            .find(|work| !work.status().terminal())
            .cloned();
        let created = active.is_none();
        let work = match active {
            Some(work) => work,
            None => {
                let objective = objective.ok_or(DriverError::NoWork)?;
                self.set_work_with(objective, WorkPolicy::Single, commands.len())?;
                self.select_plan_with(objective, commands, None)?;
                self.work_state()?
            }
        };
        let plan = work
            .selected_plan()
            .and_then(|id| self.writer.state().plans.get(&id))
            .ok_or(DriverError::NoWork)?;
        if work.policy() == WorkPolicy::Single
            && plan
                .checks
                .iter()
                .map(|check| check.command.as_str())
                .ne(commands.iter().map(String::as_str))
        {
            return Err(DriverError::Import(
                "completion checks changed while Work was active".to_string(),
            ));
        }
        match work.status().stage() {
            Some(WorkStage::Plan) if work.policy() == WorkPolicy::Single && !created => {
                return Err(DriverError::Import(
                    "accept the selected Plan before implementation".to_string(),
                ));
            }
            Some(WorkStage::Plan) => self.enter_execute()?,
            Some(WorkStage::Execute) => {}
            Some(WorkStage::Verify) => {
                return Err(DriverError::Import(
                    "Work is still in Verify after an interrupted turn".to_string(),
                ));
            }
            None => return Err(DriverError::NoWork),
        }
        Ok(())
    }

    fn enter_execute(&mut self) -> Result<(), DriverError> {
        let session = self.session();
        let mut state = self.work_state()?;
        let plan = state.selected_plan().ok_or(DriverError::NoWork)?;
        let observation = self.observe_workspace()?;
        let base = observation.id();
        let scope = self.scope()?.clone();
        let head = self.request_head()?.clone();
        let policy = VerificationPolicy {
            id: super::id::VerificationPolicyId::new(session, self.take()),
            scope: scope.clone(),
            checks: state.acceptance_checks.clone(),
            artifacts: Items::new(scope.artifacts().to_vec())?,
            coverage_rules: Items::new(scope.artifacts().iter().map(|item| item.rule).collect())?,
            reviewer: ReviewerHead {
                endpoint: head.endpoint.clone(),
                model: head.model.clone(),
                request_format: head.request_format,
                token_limit: 1_000,
            },
            repair_limit: 3,
        };
        let epoch = ContextEpoch {
            id: super::id::ContextEpochId::new(session, self.take()),
            head,
            stage: Some(WorkStage::Execute),
            base: Items::new(self.current_entries())?,
        };
        let mut items = state.items.as_slice().to_vec();
        let active = items.first().map(|item| item.id);
        if let Some(item) = items.first_mut() {
            item.disposition = ItemDisposition::Active;
        }
        state.revision = WorkRevision::new(state.revision().get().saturating_add(1))?;
        state.status = WorkStatus::Active(WorkStage::Execute);
        state.selected_plan = Some(plan);
        state.items = Items::new(items)?;
        state.active_item = active;
        state.scope = Some(scope.id());
        state.base = Some(base);
        state.verification = Some(policy);
        state.projection_owner = WorkProjection::Epoch(epoch.id);
        state.validate()?;
        let observation_fact = super::id::FactId::new(session, self.take());
        let work_fact = super::id::FactId::new(session, self.take());
        let epoch_fact = super::id::FactId::new(session, self.take());
        self.append(
            None,
            vec![
                Fact {
                    id: observation_fact,
                    data: FactData::WorkspaceObserved { observation },
                },
                Fact {
                    id: work_fact,
                    data: FactData::WorkSet {
                        work: Box::new(state),
                    },
                },
                Fact {
                    id: epoch_fact,
                    data: FactData::ContextEpochOpened { epoch },
                },
            ],
        )
    }
}

fn plan_checks(
    markdown: &str,
    policy: WorkPolicy,
    commands: &[String],
) -> Result<Vec<(String, Option<RequiredObservation>)>, DriverError> {
    let mut checks = commands
        .iter()
        .cloned()
        .map(|command| (command, None))
        .collect();
    if policy != WorkPolicy::Steps {
        return Ok(checks);
    }
    let steps = crate::steps::parse(markdown)
        .map_err(|error| DriverError::Import(format!("numbered Work plan is invalid: {error}")))?;
    for command in steps.iter().flat_map(|step| &step.checks) {
        push_check(&mut checks, command.clone(), None);
    }
    let contract = crate::steps::contract(markdown)
        .map_err(|error| DriverError::Import(format!("Work contract is invalid: {error}")))?;
    for item in contract.items {
        match item {
            crate::steps::Item::File(path) => {
                let observation = RequiredObservation::File(Text::new(&path)?);
                push_check(
                    &mut checks,
                    format!("file exists: {path}"),
                    Some(observation),
                );
            }
            crate::steps::Item::Stdout { command, expected } => {
                let observation = RequiredObservation::StdoutContains(Text::new(&expected)?);
                push_check(&mut checks, command, Some(observation));
            }
        }
    }
    Ok(checks)
}

fn push_check(
    checks: &mut Vec<(String, Option<RequiredObservation>)>,
    command: String,
    observation: Option<RequiredObservation>,
) {
    if !checks
        .iter()
        .any(|known| known.0 == command && known.1 == observation)
    {
        checks.push((command, observation));
    }
}
