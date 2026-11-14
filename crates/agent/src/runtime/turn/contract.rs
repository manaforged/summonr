use super::*;

impl<P: Provider> Loop<P> {
    pub(super) async fn adopt_contract(
        &mut self,
        objective: &str,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Option<Decision>> {
        if self.verify.is_some() || !self.session.completion_checks()?.is_empty() {
            return Ok(None);
        }
        let contract = match verify::contract(&self.workspace) {
            Ok(Some(contract)) => contract,
            Ok(None) => return Ok(None),
            Err(error) => return Ok(Some(revise(&format!("it does not load: {error}")))),
        };
        if let Some(reason) = self.red_first(&contract.checks, events, emit).await? {
            return Ok(Some(revise(&reason)));
        }
        let judged = goal::contract(self, objective, &contract, usage).await;
        self.sync_usage(emit);
        if let Err(reason) = judged {
            return Ok(Some(revise(&reason)));
        }
        let protected = contract
            .protect
            .iter()
            .map(|path| (path.clone(), verify::digest(&self.workspace, path)))
            .collect();
        self.session.adopt_checks(&contract.checks, protected)?;
        drop(std::fs::remove_file(verify::contract_path(&self.workspace)));
        let list = contract
            .checks
            .iter()
            .map(|command| format!("`{command}`"))
            .collect::<Vec<_>>()
            .join(" then ");
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!("contract frozen: {list}"),
            },
        )?;
        Ok(Some(Decision::GoalContinue(format!(
            "The contract is frozen. After each turn the harness runs {list}; the goal ends when every command passes. The files under `protect` must stay as they are."
        ))))
    }

    async fn red_first(
        &mut self,
        commands: &[String],
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<String>> {
        for command in commands {
            let result = self.tools.run_check(command, None).await;
            if !verify::outcome(command, &result).passed {
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("contract: `{command}` fails before the change"),
                    },
                )?;
                return Ok(None);
            }
        }
        Ok(Some(
            "every command already passes, so none of them can show the change. Add a command that fails until the objective is met; if the change is already made, revert it first".to_string(),
        ))
    }

    pub(super) fn protected_drift(&self) -> Option<Outcome> {
        let path = self
            .session
            .protected()
            .into_iter()
            .find(|(path, digest)| verify::digest(&self.workspace, path) != *digest)?
            .0;
        Some(Outcome {
            command: "protected files".to_string(),
            passed: false,
            output: format!(
                "`{path}` changed after the contract froze; restore it. The contract's tests and inputs are fixed for this goal."
            ),
        })
    }
}

fn revise(reason: &str) -> Decision {
    Decision::GoalContinue(format!(
        "Revise the contract in .summonr/contract.toml: {reason}. To let the judge decide instead, delete the file."
    ))
}
