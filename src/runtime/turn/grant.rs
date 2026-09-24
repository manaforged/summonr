use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn verdict(
        &mut self,
        checks: &mut Vec<Outcome>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Verdict> {
        let verdict = self
            .session
            .prepare_verdict(checks.iter().all(|check| check.passed))
            .map_err(super::session_err)?;
        if let Some(output) = &verdict.diagnostic {
            let command = "workspace candidate unchanged".to_string();
            self.push_event(
                events,
                emit,
                Event::Verify {
                    command: command.clone(),
                    passed: false,
                },
            )?;
            checks.push(Outcome {
                command,
                passed: false,
                output: output.clone(),
            });
        }
        Ok(verdict)
    }

    pub(super) async fn approve(
        &mut self,
        detail: String,
        persist: bool,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<GrantAnswer> {
        let Some(shared) = self.steer_shared.clone() else {
            return Ok(GrantAnswer::Deny);
        };
        let detail: String = detail.chars().take(160).collect();
        self.push_event(
            events,
            emit,
            Event::Approval {
                detail: Some(detail),
                persist,
            },
        )?;
        let deadline = Instant::now() + crate::limits::GRANT_WAIT;
        let decision = loop {
            if Instant::now() >= deadline {
                break GrantAnswer::Deny;
            }
            let answer = shared.lock().ok().and_then(|mut queue| queue.pop_front());
            if let Some(input) = answer {
                break match parse_decision(input.text.trim(), persist) {
                    Some(decision) => decision,
                    None => {
                        self.steer.push_front(input);
                        GrantAnswer::Deny
                    }
                };
            }
            tokio::time::sleep(crate::limits::GRANT_POLL).await;
        };
        self.push_event(
            events,
            emit,
            Event::Approval {
                detail: None,
                persist: false,
            },
        )?;
        Ok(decision)
    }

    pub(super) async fn with_grant(
        &mut self,
        mut output: Result<ToolOutput>,
        call: &ToolCall,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Result<ToolOutput>> {
        for _ in 0..4 {
            let Some(grant) = needed_grant(&output) else {
                break;
            };
            if self.tools.is_read_only() && !matches!(grant, Grant::Network) {
                break;
            }
            let persist = matches!(grant, Grant::Write(_));
            let question = grant.prompt();
            if self.asked.contains(&grant) {
                break;
            }
            if !self.manual
                && !matches!(
                    grant,
                    Grant::Guarded(_) | Grant::Command(_) | Grant::Danger(_) | Grant::Edit(_)
                )
            {
                self.apply_grant(&grant, false, events, emit)?;
                output = self.tools.execute_outcome(call).await;
                continue;
            }
            if let Grant::Danger(command) = &grant
                && !self.manual
            {
                let reason = crate::policy::danger(command, &self.workspace)
                    .unwrap_or("it looked destructive");
                match self.review(command, reason).await {
                    super::approval::Verdict::Allow => {
                        self.push_event(
                            events,
                            emit,
                            Event::Note {
                                text: format!("reviewer allowed: {command}"),
                            },
                        )?;
                        self.tools.approve_once();
                        output = self.tools.execute_outcome(call).await;
                        continue;
                    }
                    super::approval::Verdict::Block(why) => {
                        output = Ok(blocked(command, &why));
                        self.push_event(
                            events,
                            emit,
                            Event::Note {
                                text: format!("reviewer blocked {command}: {why}"),
                            },
                        )?;
                        break;
                    }
                    super::approval::Verdict::Ask(why) if self.steer_shared.is_none() => {
                        output = Ok(blocked(command, &why));
                        break;
                    }
                    super::approval::Verdict::Ask(_) => {}
                }
            }
            if let Grant::Edit(setting) = &grant
                && self.edit_requested(setting).await
            {
                self.apply_grant(&grant, false, events, emit)?;
                output = self.tools.execute_outcome(call).await;
                continue;
            }
            if self.steer_shared.is_none() {
                break;
            }
            self.asked.push(grant.clone());
            match self.approve(question, persist, events, emit).await? {
                GrantAnswer::Deny => break,
                GrantAnswer::Allow => self.apply_grant(&grant, false, events, emit)?,
                GrantAnswer::Always => self.apply_grant(&grant, true, events, emit)?,
            }
            output = self.tools.execute_outcome(call).await;
        }
        Ok(output)
    }

    pub(super) fn apply_grant(
        &mut self,
        grant: &Grant,
        standing: bool,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        match grant {
            Grant::Write(path) | Grant::Guarded(path) => {
                self.allow_path(&path.to_string_lossy())?;
                let mut text = format!("writes allowed under {} for this session", path.display());
                if standing {
                    match crate::config::save_sandbox_write(
                        &crate::config::path()?,
                        &crate::config::sandbox_entry(path),
                    ) {
                        Ok(()) => {
                            text = format!(
                                "writes allowed under {} and saved to [sandbox] write",
                                path.display()
                            );
                        }
                        Err(error) => {
                            text = format!("{text}; standing grant not saved: {error}");
                        }
                    }
                }
                self.push_event(events, emit, Event::Note { text })?;
            }
            Grant::Danger(_) => self.tools.approve_once(),
            Grant::Edit(setting) => {
                self.tools.allow_edit(setting);
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("{setting} allowed for this task"),
                    },
                )?;
            }
            Grant::Command(command) => {
                self.tools.approve_once();
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("running this command without the sandbox: {command}"),
                    },
                )?;
            }
            Grant::Network => {
                self.set_network(true)?;
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: "network allowed for this session".to_string(),
                    },
                )?;
            }
        }
        Ok(())
    }
}
