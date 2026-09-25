use super::record::*;
use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn task_spawn(&mut self, args: Args, budget: &Budget) -> Result<Execution> {
        let run = self.task_start(args, budget, None)?;
        let record = run.record.clone();
        let path = run.child.session.path().to_path_buf();
        let id = record.id.to_string();
        let handle = tokio::spawn(run.execute(self.mailbox.0.clone()));
        self.spawned.insert(
            id.clone(),
            Spawned {
                record: record.clone(),
                path,
                handle,
                started: Instant::now(),
                last: String::new(),
                reserved_tokens: budget.max_tokens.unwrap_or_default(),
            },
        );
        Ok(Execution {
            output: ToolOutput {
                text: format!(
                    "started child {id}\naccess: {}\nassignment: {}\nstate: running\npoll with task {{\"child\": \"{id}\"}}; stop with kill true",
                    access_name(record.access),
                    ellipsize(&record.assignment, LAST_CHARS)
                ),
                success: true,
                mutated: false,
                grant: None,
            },
            usage: Usage::default(),
            millis: 0,
        })
    }

    pub(super) fn task_start(
        &mut self,
        args: Args,
        budget: &Budget,
        tree: Option<Tree>,
    ) -> Result<Run<P>> {
        let mut guard = Guard { tree };
        let result = self.task_start_in(args, budget, guard.tree.clone());
        if result.is_ok() {
            drop(guard.disarm());
        }
        result
    }

    pub(super) fn task_start_in(
        &mut self,
        args: Args,
        budget: &Budget,
        tree: Option<Tree>,
    ) -> Result<Run<P>> {
        let role = self.round_role();
        let provider = self.seat(role).0.child().ok_or_else(|| {
            Error::Denied("the active provider cannot create children".to_string())
        })?;
        let model = self.seat(role).1.to_string();
        let home = log::home_of(self.session.path())
            .ok_or_else(|| Error::Config("the parent session has no durable home".to_string()))?;
        let workspace = tree
            .as_ref()
            .map_or_else(|| self.workspace.clone(), |tree| tree.path.clone());
        let driver = SessionDriver::open_child(
            home,
            &workspace,
            &model,
            &self.system_prompt,
            self.session.id(),
        )
        .map_err(super::session_err)?;
        let instruction = match args.access {
            Access::Implement => {
                "Edit this workspace. You may build, test, and lint here. Background commands stay with the parent."
            }
            Access::Research => "This assignment is read-only. Do not change the workspace.",
        };
        let mut child = Self::with_parts(
            provider,
            model.clone(),
            &workspace,
            format!(
                "{}\n\nHarness: child. Finish only the assigned task. {instruction}",
                self.system_prompt
            ),
            driver,
            Policy::builtin(),
            None,
        )?;
        child.tools = self.tools.child_at(&workspace, &child.session_id())?;
        if args.access == Access::Research {
            child.tools.set_read_only(true);
        }
        child.depth = 1;
        child.mode = self.mode;
        child.context_window = self.context_window;
        child.context_sample = self.context_sample;
        child.effort = self.effort_for(role);
        child.price = self.price_for(role);
        child = child.with_budget(budget.clone())?;

        let record = ChildRecord {
            version: 1,
            id: child.session.id(),
            parent: self.session.id(),
            assignment: args.assignment.clone(),
            model: model.clone(),
            workspace,
            depth: 1,
            access: args.access,
            call: Some(args.call.clone()),
            state: ChildState::Running,
        };
        self.session
            .start_child(record.clone())
            .map_err(super::session_err)?;
        Ok(Run {
            child,
            record,
            guard: Guard { tree },
            assignment: args.assignment,
            deadline: budget.max_secs.map(crate::limits::seconds),
            started: Instant::now(),
        })
    }

    pub(super) fn task_reconcile(&self, done: &mut [(Access, Result<Done>)]) {
        let failed = done.iter().any(|(access, done)| {
            *access == Access::Implement
                && match done {
                    Ok(done) => !matches!(done.record.state, ChildState::Completed(_)),
                    Err(_) => true,
                }
        });
        if failed {
            for (access, done) in done {
                if *access == Access::Implement
                    && let Ok(done) = done
                    && matches!(done.record.state, ChildState::Completed(_))
                {
                    fail_record(
                        &mut done.record,
                        "writable child changes were not reconciled because another implement child did not complete; the worktree was kept",
                    );
                }
            }
            return;
        }
        let children = done
            .iter()
            .filter_map(|(access, done)| {
                if *access != Access::Implement {
                    return None;
                }
                let done = done.as_ref().ok()?;
                Some((done.tree.clone()?, done.paths.clone()))
            })
            .collect::<Vec<_>>();
        if children.is_empty() {
            return;
        }
        match worktree::reconcile_children(&self.workspace, &children) {
            Ok(warnings) if warnings.is_empty() => {}
            Ok(warnings) => {
                let warning = warnings.join("; ");
                for (access, done) in done {
                    if *access == Access::Implement
                        && let Ok(done) = done
                    {
                        note_record(&mut done.record, &warning);
                    }
                }
            }
            Err(error) => {
                let diagnostic = error.to_string();
                for (access, done) in done {
                    if *access == Access::Implement
                        && let Ok(done) = done
                    {
                        fail_record(&mut done.record, &diagnostic);
                    }
                }
            }
        }
    }

    pub(super) fn task_settle(&mut self, mut done: Done) -> Result<Execution> {
        if !done.settled {
            self.task_record(&mut done)?;
        }
        let record = done.record;
        let text = serde_json::to_string(&record)
            .map_err(|error| Error::Config(format!("cannot encode child receipt: {error}")))?;
        let success = matches!(record.state, ChildState::Completed(_));
        let mutated = record_paths(&record) > 0;
        Ok(Execution {
            output: ToolOutput {
                text,
                success,
                mutated,
                grant: None,
            },
            usage: done.usage,
            millis: done.millis,
        })
    }

    pub(super) fn task_budget(&self, children: usize) -> Result<Budget> {
        let children = u64::try_from(children)
            .map_err(|error| Error::Config(format!("task batch size is invalid: {error}")))?;
        if children == 0 {
            return Err(Error::Config("task batch is empty".to_string()));
        }
        let tokens = self.budget.max_tokens.map(|limit| {
            limit
                .saturating_sub(self.spent_tokens)
                .saturating_sub(self.child_budget_reserved())
                / children
        });
        let secs = self
            .budget
            .max_secs
            .map(|limit| limit.saturating_sub(self.elapsed_secs()));
        if tokens == Some(0) || secs == Some(0) {
            return Err(Error::Denied(
                "the parent budget has no capacity for a child".to_string(),
            ));
        }
        Ok(Budget {
            max_tokens: tokens,
            max_secs: secs,
        })
    }

    pub(super) fn result(
        &self,
        text: &str,
        rounds: usize,
        usage: Usage,
        diagnostic: Option<&str>,
        access: Access,
    ) -> ChildResult {
        let paths = match access {
            Access::Implement => Self::touched_at(self.session.path()),
            Access::Research => Vec::new(),
        };
        let total = paths.len();
        ChildResult {
            text: text.to_string(),
            changed_paths: paths,
            changed_paths_total: total,
            rounds,
            usage,
            billed_tokens: self.session.spent_tokens(),
            elapsed_millis: self.session.elapsed_millis(),
            diagnostic: diagnostic.map(str::to_string),
        }
    }
}
