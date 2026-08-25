//! Provider and tool effect transitions.

use super::*;

impl SessionDriver {
    /// Record a provider attempt against the open operation.
    pub(crate) fn start_provider(
        &mut self,
        request: &[u8],
        exposure: Exposure,
    ) -> Result<super::id::ProviderAttemptId, DriverError> {
        self.start_provider_as(request, exposure, ProviderPurpose::AgentRound)
    }

    /// Record a compaction-summary provider attempt.
    pub(crate) fn start_summary(
        &mut self,
        request: &[u8],
        exposure: Exposure,
    ) -> Result<super::id::ProviderAttemptId, DriverError> {
        self.start_provider_as(request, exposure, ProviderPurpose::CompactionSummary)
    }

    /// Record the harness-owned Goal completion decision.
    pub(crate) fn start_judge(
        &mut self,
        request: &[u8],
        exposure: Exposure,
    ) -> Result<super::id::ProviderAttemptId, DriverError> {
        self.start_provider_as(request, exposure, ProviderPurpose::GoalJudge)
    }

    fn start_provider_as(
        &mut self,
        request: &[u8],
        exposure: Exposure,
        purpose: ProviderPurpose,
    ) -> Result<super::id::ProviderAttemptId, DriverError> {
        let session = self.session();
        let operation = self.open_operation()?;
        let epoch = self.epoch()?;
        let through = self.through();
        let attempt = super::id::ProviderAttemptId::new(session, self.take());
        let result = super::id::ProviderResultId::new(session, self.take());
        let intent = ProviderIntent {
            attempt,
            result,
            operation,
            epoch,
            purpose,
            through,
            request_bytes: u32::try_from(request.len()).unwrap_or(u32::MAX),
            request_digest: Digest::hash(b"mage:provider-request:v1", request),
            exposure,
        };
        intent.validate()?;
        let facts = vec![Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::ProviderStarted { intent },
        }];
        self.append(None, facts)?;
        Ok(attempt)
    }

    /// Close a provider round with a completed outcome.
    pub(crate) fn finish_provider(
        &mut self,
        attempt: super::id::ProviderAttemptId,
        text: &str,
        reasoning: &str,
        usage: Usage,
    ) -> Result<(), DriverError> {
        let result = self.open_provider_result()?;
        let outcome = ProviderOutcome::Completed {
            message: ProviderMessage {
                text: Text::new(text)?,
                reasoning: Text::new(reasoning)?,
                tool_calls: Items::default(),
            },
            reported_usage: usage.clone(),
        };
        let facts = self.provider_finish_facts(attempt, result, &[], text, outcome, usage)?;
        self.append(None, facts)?;
        Ok(())
    }

    /// Close a Goal judge call without adding its private JSON to chat history.
    pub(crate) fn finish_judge(
        &mut self,
        attempt: super::id::ProviderAttemptId,
        text: &str,
        usage: Usage,
    ) -> Result<(), DriverError> {
        let result = self.open_provider_result()?;
        let outcome = ProviderOutcome::Completed {
            message: ProviderMessage {
                text: Text::new(text)?,
                reasoning: Text::default(),
                tool_calls: Items::default(),
            },
            reported_usage: usage.clone(),
        };
        let facts = self.provider_finish_facts(attempt, result, &[], text, outcome, usage)?;
        self.append(None, facts)?;
        Ok(())
    }

    /// Record one harness-owned completion check while its operation is open.
    pub(crate) fn record_completion_check(
        &mut self,
        check: CompletionCheck,
        command: &str,
        passed: bool,
        output: &str,
    ) -> Result<EvidenceRef, DriverError> {
        let session = self.session();
        let operation = self.open_operation()?;
        let source = super::id::FactId::new(session, self.take());
        let fact = Fact {
            id: source,
            data: FactData::CompletionCheckRecorded {
                operation,
                check,
                command: Text::new(command)?,
                passed,
                output: Text::new(output)?,
            },
        };
        let evidence = EvidenceRef {
            id: super::id::EvidenceId::new(session, self.take()),
            source,
            digest: completion_digest(command, passed, output),
            summary: Text::new(if passed {
                "Completion check passed."
            } else {
                "Completion check failed."
            })?,
        };
        self.append(None, vec![fact])?;
        Ok(evidence)
    }

    /// Close a provider round and plan one tool batch in provider order.
    pub(crate) fn finish_provider_with_tools(
        &mut self,
        attempt: super::id::ProviderAttemptId,
        text: &str,
        reasoning: &str,
        usage: Usage,
        tools: &[(&str, &str, &str)],
    ) -> Result<Vec<ToolCallId>, DriverError> {
        let session = self.session();
        let result = self.open_provider_result()?;
        let planned = self.plan_tools(session, result, tools)?;
        let payloads = planned
            .iter()
            .zip(tools)
            .map(|(tool, (name, arguments, _))| {
                Ok(super::effect::ProviderToolCall {
                    provider_id: tool.provider_id.clone(),
                    name: Text::new(*name)?,
                    arguments: Text::new(*arguments)?,
                })
            })
            .collect::<Result<Vec<_>, BoundError>>()?;
        let outcome = ProviderOutcome::Completed {
            message: ProviderMessage {
                text: Text::new(text)?,
                reasoning: Text::new(reasoning)?,
                tool_calls: Items::new(payloads)?,
            },
            reported_usage: usage.clone(),
        };
        let facts = self.provider_finish_facts(attempt, result, &planned, text, outcome, usage)?;
        self.append(None, facts)?;
        Ok(planned.into_iter().map(|tool| tool.id).collect())
    }

    /// Close a provider attempt that did not complete.
    pub(crate) fn fail_provider(
        &mut self,
        attempt: super::id::ProviderAttemptId,
        kind: ProviderFailureKind,
        diagnostic: &str,
        live_millis: u64,
    ) -> Result<(), DriverError> {
        let result = self.open_provider_result()?;
        let usage = Usage {
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            live_millis: Some(live_millis),
        };
        let outcome = ProviderOutcome::Failed {
            kind,
            diagnostic: Text::new(diagnostic)?,
            partial_usage: usage.clone(),
        };
        let facts = self.provider_finish_facts(attempt, result, &[], "", outcome, usage)?;
        self.append(None, facts)?;
        Ok(())
    }

    pub(super) fn provider_finish_facts(
        &mut self,
        attempt: super::id::ProviderAttemptId,
        result: super::id::ProviderResultId,
        tools: &[ToolCall],
        text: &str,
        outcome: ProviderOutcome,
        usage: Usage,
    ) -> Result<Vec<Fact>, DriverError> {
        let intent = match self.writer.state().providers.get(&attempt) {
            Some(ProviderState::Started(intent)) => intent.clone(),
            _ => return Err(DriverError::NoProvider),
        };
        let add_context = intent.purpose != ProviderPurpose::GoalJudge
            && matches!(&outcome, ProviderOutcome::Completed { .. });
        let settlement = match &outcome {
            ProviderOutcome::Completed { .. } => MeterSettlement::Known(usage),
            ProviderOutcome::Failed { .. } => MeterSettlement::Estimated(Usage {
                input_tokens: Some(
                    usage
                        .input_tokens
                        .unwrap_or(intent.exposure.max_input_tokens),
                ),
                output_tokens: Some(
                    usage
                        .output_tokens
                        .unwrap_or(intent.exposure.max_output_tokens),
                ),
                ..usage
            }),
            ProviderOutcome::Interrupted { .. } => MeterSettlement::Estimated(usage),
        };
        let session = self.session();
        let mut facts = vec![
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ProviderFinished {
                    attempt,
                    result,
                    outcome,
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::MeterSettled {
                    owner: MeterOwner::Provider(attempt),
                    settlement,
                },
            },
        ];
        if add_context {
            let mut entry = ContextEntry {
                id: super::id::ContextEntryId::new(session, self.take()),
                epoch: self.epoch()?,
                role: ContextRole::Assistant,
                source: ContextSource::Provider(result),
                payload: Text::new(text)?,
                digest: Digest::default(),
            };
            entry.digest = entry.calculated_digest();
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            });
        }
        facts.extend(tools.iter().cloned().map(|call| Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::ToolCallPlanned { call },
        }));
        Ok(facts)
    }

    fn plan_tools(
        &mut self,
        session: SessionId,
        provider_result: super::id::ProviderResultId,
        tools: &[(&str, &str, &str)],
    ) -> Result<Vec<ToolCall>, DriverError> {
        let batch = super::id::ToolBatchId::new(session, self.take());
        let mut planned = Vec::new();
        for (index, (_name, _arguments, provider_id)) in tools.iter().enumerate() {
            let position =
                u16::try_from(index).map_err(|_error| super::effect::EffectError::ToolOrder)?;
            planned.push(ToolCall {
                id: super::id::ToolCallId::new(session, self.take()),
                batch,
                result: super::id::ToolResultId::new(session, self.take()),
                provider_result,
                provider_id: Text::new(*provider_id)?,
                position,
            });
        }
        Ok(planned)
    }

    /// Record a tool start.
    pub(crate) fn start_tool(
        &mut self,
        call: ToolCallId,
        arguments: &str,
    ) -> Result<(), DriverError> {
        let session = self.session();
        self.writer
            .state()
            .tools
            .get(&call)
            .ok_or(DriverError::NoTool)?;
        let attempt = super::id::ToolAttemptId::new(session, self.take());
        let intent = ToolIntent {
            attempt,
            call,
            normalized_arguments: Text::new(arguments)?,
        };
        let facts = vec![Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::ToolStarted { intent },
        }];
        self.append(None, facts)?;
        Ok(())
    }

    /// Record every tool terminal in one batch, then project context in
    /// provider-call order.
    pub(crate) fn finish_tools(
        &mut self,
        results: &[(ToolCallId, &str, bool, Usage)],
    ) -> Result<(), DriverError> {
        if results.is_empty() {
            return Ok(());
        }
        let session = self.session();
        let epoch = self.epoch()?;
        let mut prepared = Vec::new();
        for (call, output, success, usage) in results {
            let tool = self
                .writer
                .state()
                .tools
                .get(call)
                .ok_or(DriverError::NoTool)?;
            let intent = tool.intent.as_ref().ok_or(DriverError::NoTool)?;
            let attempt = intent.attempt;
            prepared.push((
                *call,
                tool.call.result,
                attempt,
                *output,
                *success,
                usage.clone(),
            ));
        }
        let mut facts = Vec::new();
        for (call, result, attempt, output, success, usage) in &prepared {
            let outcome = if *success {
                ToolOutcome::Completed {
                    output: Text::new(*output)?,
                }
            } else {
                ToolOutcome::Failed {
                    diagnostic: Text::new(*output)?,
                }
            };
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ToolFinished {
                    result: ToolResult {
                        id: *result,
                        call: *call,
                        attempt: Some(*attempt),
                        outcome,
                    },
                },
            });
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::MeterSettled {
                    owner: MeterOwner::Tool(*attempt),
                    settlement: MeterSettlement::Known(usage.clone()),
                },
            });
        }
        for (_call, result, _attempt, output, _success, _usage) in &prepared {
            let mut entry = ContextEntry {
                id: super::id::ContextEntryId::new(session, self.take()),
                epoch,
                role: ContextRole::Tool,
                source: ContextSource::Tool(*result),
                payload: Text::new(*output)?,
                digest: Digest::default(),
            };
            entry.digest = entry.calculated_digest();
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            });
        }
        self.append(None, facts)?;
        Ok(())
    }
}
