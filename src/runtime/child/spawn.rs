use super::brief::{agent_brief, instruction, rebase_paths};
use super::record::*;
use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn task_spawn(&mut self, args: Args, budget: &Budget) -> Result<Execution> {
        let tree = child_tree(&self.workspace, args.access)?;
        let run = self.task_start(args, budget, tree)?;
        let record = run.record.clone();
        let path = run.child.session.path().to_path_buf();
        let id = record.id.to_string();
        let handle = tokio::spawn(run.execute(self.events.0.clone()));
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
                    record.access.name(),
                    ellipsize(&record.assignment, LAST_CHARS)
                ),
                success: true,
                mutated: false,
                grant: None,
                images: Vec::new(),
                patch: None,
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
        let driver = SessionDriver::open(
            home,
            &workspace,
            &model,
            &self.system_prompt,
            Some(self.session.id()),
        )
        .map_err(super::session_err)?;
        let (opener, tools) = self.child_opener(&args, tree.as_ref(), &workspace)?;
        let mut child = Self::with_parts(
            provider,
            model.clone(),
            &workspace,
            self.system_prompt.clone(),
            driver,
            Policy::builtin(),
            None,
        )?;
        child.tools = self.child_tools(args.access, tools.as_deref(), &workspace, &child)?;
        child.depth = 1;
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
            background: args.background,
            call: Some(args.call.clone()),
            state: ChildState::Running,
        };
        self.session
            .start_child(record.clone())
            .map_err(super::session_err)?;
        drop(self.events.0.send(Event::Child {
            id: record.id.to_string(),
            event: Box::new(Event::Spawned {
                assignment: record.assignment.clone(),
                access: record.access,
                call: args.call.clone(),
                background: args.background,
                workspace: record.workspace.to_string_lossy().into_owned(),
                agent: args.agent.clone().unwrap_or_default(),
            }),
        }));
        Ok(Run {
            child,
            record,
            guard: Guard { tree },
            assignment: opener,
            deadline: budget.max_secs.map(crate::limits::seconds),
            started: Instant::now(),
        })
    }

    fn child_opener(
        &self,
        args: &Args,
        tree: Option<&Tree>,
        workspace: &Path,
    ) -> Result<(String, Option<Vec<String>>)> {
        let instruction = instruction(args.access, tree.is_some());
        let (brief, tools) = agent_brief(args.agent.as_deref(), &self.workspace)?;
        let assignment = tree.map_or_else(
            || args.assignment.clone(),
            |tree| rebase_paths(&args.assignment, &self.workspace, &tree.path),
        );
        let opener = format!(
            "Harness: child. Finish only the assigned task. {instruction} Your working directory is {}.\n\n{brief}{assignment}",
            workspace.display()
        );
        Ok((opener, tools))
    }

    fn child_tools(
        &self,
        access: Access,
        tools: Option<&[String]>,
        workspace: &Path,
        child: &Self,
    ) -> Result<crate::tools::ToolRegistry> {
        let mut registry = self.tools.child_at(workspace, &child.session_id())?;
        if access == Access::Research || self.tools.is_read_only() {
            registry.contain_read_only();
        }
        if let Some(tools) = tools {
            registry.restrict_to(tools);
        }
        Ok(registry)
    }

    pub(super) fn task_reconcile(&self, done: &mut [(Access, Result<Done>)]) {
        for (_, result) in done.iter_mut() {
            let Ok(done) = result else {
                continue;
            };
            if done.record.access != Access::Implement {
                continue;
            }
            if matches!(done.record.state, ChildState::Completed(_)) {
                continue;
            }
            release_unfinished(done);
        }
        let mut picked = Vec::new();
        for (index, (access, result)) in done.iter().enumerate() {
            if *access != Access::Implement {
                continue;
            }
            let Ok(done) = result else {
                continue;
            };
            if !matches!(done.record.state, ChildState::Completed(_)) {
                continue;
            }
            let Some(tree) = done.tree.clone() else {
                continue;
            };
            picked.push((index, tree, done.paths.clone()));
        }
        if picked.is_empty() {
            return;
        }
        let children = picked
            .iter()
            .map(|(_, tree, paths)| (tree.clone(), paths.clone()))
            .collect::<Vec<_>>();
        match worktree::reconcile_children(&self.workspace, &children) {
            Ok(landings) => {
                for ((index, _, _), landing) in picked.into_iter().zip(landings) {
                    if let Ok(done) = done[index].1.as_mut() {
                        done.record.record_landing(landing);
                    }
                }
            }
            Err(error) => {
                let diagnostic = error.to_string();
                for done in implemented(done) {
                    done.record.fail_record(&diagnostic);
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
        let mutated = record.record_paths() > 0;
        Ok(Execution {
            output: ToolOutput {
                text,
                success,
                mutated,
                grant: None,
                images: Vec::new(),
                patch: None,
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
                .saturating_sub(self.run_tokens())
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
            kept: None,
        }
    }
}

pub(super) fn child_tree(workspace: &Path, access: Access) -> Result<Option<Tree>> {
    if access != Access::Implement || !worktree::in_git(workspace) {
        return Ok(None);
    }
    worktree::open_delta(workspace).map(Some)
}

fn release_unfinished(done: &mut Done) {
    let Some(tree) = done.tree.clone() else {
        return;
    };
    if tree.snapshot.is_none() {
        return;
    }
    if tree.unchanged().unwrap_or(false) {
        drop(tree.remove_child());
        done.tree = None;
        return;
    }
    let note = format!(
        "the delta at {} holds changes and was kept",
        tree.path.display()
    );
    let Some(result) = result_mut(&mut done.record.state) else {
        return;
    };
    result.diagnostic = Some(match result.diagnostic.take() {
        Some(existing) => format!("{existing}; {note}"),
        None => note,
    });
}

impl ChildRecord {
    fn record_landing(&mut self, landing: worktree::Landing) {
        if let Some(kept) = landing.kept {
            if landing.paths.is_empty() && landing.held.is_none() {
                self.fail_record(&kept);
                return;
            }
            self.note_record(&kept);
        }
        let Some(result) = result_mut(&mut self.state) else {
            return;
        };
        result.kept = landing.held;
        result.changed_paths_total = landing.paths.len();
        result.changed_paths = landing.paths;
    }
}

fn implemented(done: &mut [(Access, Result<Done>)]) -> impl Iterator<Item = &mut Done> {
    done.iter_mut()
        .filter(|(access, _)| *access == Access::Implement)
        .filter_map(|(_, done)| done.as_mut().ok())
}

#[cfg(test)]
#[path = "spawn_tests.rs"]
mod tests;
