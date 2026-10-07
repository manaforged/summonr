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
        let _blocked = self.herdr.blocked(&detail);
        self.push_event(
            events,
            emit,
            Event::Approval {
                detail: Some(detail),
                persist,
            },
        )?;
        let decision = match self.answer(&shared).await {
            Some(input) => match parse_decision(input.text.trim(), persist) {
                Some(decision) => decision,
                None => {
                    self.steer.push_front(input);
                    GrantAnswer::Deny
                }
            },
            None => GrantAnswer::Deny,
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
        let mut asked = Vec::new();
        for _ in 0..4 {
            let Some(grant) = needed_grant(&output) else {
                break;
            };
            if self.grant_blocked(&grant, &asked) {
                break;
            }
            if !matches!(grant, Grant::Danger(_)) {
                self.apply_grant(&grant, false, events, emit)?;
                output = self.tools.execute_outcome(call).await;
                continue;
            }
            if !self.ask_grant(grant, &mut asked, events, emit).await? {
                break;
            }
            output = self.tools.execute_outcome(call).await;
        }
        Ok(output)
    }

    fn grant_blocked(&self, grant: &Grant, asked: &[Grant]) -> bool {
        (self.tools.is_read_only() && !matches!(grant, Grant::Network)) || asked.contains(grant)
    }

    async fn ask_grant(
        &mut self,
        grant: Grant,
        asked: &mut Vec<Grant>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        if self.steer_shared.is_none() {
            return Ok(false);
        }
        let persist = matches!(grant, Grant::Write(_));
        let question = grant.prompt();
        asked.push(grant.clone());
        let standing = match self.approve(question, persist, events, emit).await? {
            GrantAnswer::Deny => return Ok(false),
            GrantAnswer::Allow => false,
            GrantAnswer::Always => true,
        };
        self.apply_grant(&grant, standing, events, emit)?;
        Ok(true)
    }

    pub(super) fn apply_grant(
        &mut self,
        grant: &Grant,
        standing: bool,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let text = match grant {
            Grant::Write(path) | Grant::Guarded(path) => self.grant_write(path, standing)?,
            Grant::Danger(_) => {
                self.tools.approve_once();
                return Ok(());
            }
            Grant::Edit(setting) => {
                self.tools.allow_edit(setting);
                format!("{setting} allowed for this task")
            }
            Grant::Command(command) => {
                self.tools.approve_once();
                format!("running this command without the sandbox: {command}")
            }
            Grant::Network => {
                self.set_network(crate::Network::Allowed)?;
                "network allowed for this session".to_string()
            }
        };
        self.push_event(events, emit, Event::Note { text })?;
        Ok(())
    }

    fn grant_write(&mut self, path: &std::path::Path, standing: bool) -> Result<String> {
        self.allow_path(&path.to_string_lossy())?;
        let text = format!("writes allowed under {} for this session", path.display());
        if !standing {
            return Ok(text);
        }
        Ok(
            match crate::config::save_sandbox_write(
                &crate::config::path()?,
                &crate::config::sandbox_entry(path),
            ) {
                Ok(()) => format!(
                    "writes allowed under {} and saved to [sandbox] write",
                    path.display()
                ),
                Err(error) => format!("{text}; standing grant not saved: {error}"),
            },
        )
    }
}
