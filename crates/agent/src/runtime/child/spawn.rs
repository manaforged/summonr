use super::brief::{agent_brief, instruction, rebase_paths};
use super::*;
use crate::durable::driver::Parent;

impl<P: Provider> Loop<P> {
    pub(super) fn task_spawn(&mut self, args: Args, budget: &Budget) -> Result<Execution> {
        let named = args.name.is_some();
        let choice = selected_workspace(named, args.workspace);
        let tree = child_tree(&self.workspace, args.access, choice)?;
        if tree.is_some() {
            self.hold_children()?;
        }
        let mut run = self.task_start(args, budget, tree)?;
        let record = run.record.clone();
        let path = run.child.session.path().to_path_buf();
        let id = record.id.to_string();
        let name = run.child.name().unwrap_or_else(|| id.clone());
        let state = run.state.clone();
        let effort = run
            .child
            .effort
            .map_or("provider default", |level| level.as_str());
        run.stay = named;
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
                state,
                stay: named,
                tool: String::new(),
            },
        );
        Ok(Execution {
            output: ToolOutput::ok(format!(
                "started child {id}\nname: {}\nmodel: {}\neffort: {}\naccess: {}\n{}\nassignment: {}\nstate: running\nyou are told when it finishes; stop with kill true",
                name,
                record.model,
                effort,
                record.access.name(),
                running_place(&record.workspace),
                ellipsize(&record.assignment, LAST_CHARS)
            )),
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
        let role = crate::workflow::Role::Implement;
        let endpoint = args.endpoint()?;
        let provider = self
            .selection(role)
            .0
            .child(endpoint.as_ref())?
            .ok_or_else(|| {
                Error::Denied("the selected provider cannot create children".to_string())
            })?;
        let model = endpoint.as_ref().map_or_else(
            || self.selection(role).1.to_string(),
            |endpoint| endpoint.model.clone(),
        );
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
            Some(Parent::Task(self.session.id())),
        )?;
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
        if let Some(set) = self.tools.skill_set().cloned() {
            child.choose_skills(set)?;
        }
        if args.access == Access::Implement {
            child.mcp = self
                .mcp
                .as_ref()
                .map(|hub| Arc::new(hub.for_child(&workspace)));
        }
        child.mail_module = self.mail_module;
        child.context_window = self.receiver_window(role);
        child.compact_tokens = self.compact_tokens;
        child.context_sample = self.context_sample;
        child.effort = self.effort_for(role);
        child.price = self.price_for(role);
        if let Some(endpoint) = &endpoint {
            child.apply_endpoint(endpoint);
            child.context_sample = None;
        }
        if let Some(text) = args.effort.as_deref() {
            child.effort = Some(
                crate::provider::Effort::parse(text)
                    .ok_or_else(|| Error::Tool(format!("task effort {text:?} is not a level")))?,
            );
        }
        child = child.with_budget(budget.clone())?;
        if let Some(name) = args.name.as_deref() {
            child.set_name(name)?;
        }
        child.roster();

        let record = ChildRecord {
            version: 1,
            id: child.session.id(),
            parent: self.session.id(),
            assignment: args.assignment.clone(),
            model: model.clone(),
            name: child.name().unwrap_or_default(),
            agent: args.agent.clone().unwrap_or_default(),
            workspace,
            depth: self.session.depth().saturating_add(1),
            access: args.access,
            background: args.background,
            call: Some(args.call.clone()),
            base: start_tree(args.access, &self.workspace, &child.session_id()),
            state: ChildState::Running,
        };
        self.session.start_child(record.clone())?;
        crate::tools::command::own_child(&record.id.to_string());
        drop(self.events.0.send(record.spawned()));
        Ok(Run {
            child,
            record,
            guard: Guard { tree },
            assignment: opener,
            deadline: budget.max_secs.map(crate::limits::seconds),
            started: Instant::now(),
            state: watch::Sender::new(ChildState::Running),
            stay: false,
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
        let stay = if args.name.is_some() {
            " Finish the assigned task, then stay. Mail to your name is the next turn."
        } else {
            " Finish only the assigned task."
        };
        let opener = format!(
            "Harness: child.{stay} {instruction} Your working directory is {}.\n\n{brief}{assignment}",
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
        if access == Access::Research {
            registry.allow_shell();
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
        let landings = worktree::reconcile_children(&self.workspace, &children);
        for ((index, _, _), landing) in picked.into_iter().zip(landings) {
            if let Ok(done) = done[index].1.as_mut() {
                done.record.record_landing(landing);
            }
        }
    }

    pub(super) fn task_budget(&self, children: usize) -> Result<Budget> {
        let children = u64::try_from(children)
            .map_err(|error| Error::Config(format!("task batch size is invalid: {error}")))?;
        if children == 0 {
            return Err(Error::Config("task batch is empty".to_string()));
        }
        let tokens = self.budget().max_tokens.map(|limit| {
            limit
                .saturating_sub(self.run_tokens())
                .saturating_sub(self.child_budget_reserved())
                / children
        });
        let secs = self
            .budget()
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
}

pub(super) fn isolated(args: &Args) -> bool {
    args.access == Access::Implement
        && selected_workspace(args.name.is_some(), args.workspace) == Workspace::Isolated
}

pub(super) fn selected_workspace(named: bool, choice: Option<Workspace>) -> Workspace {
    match choice {
        Some(choice) => choice,
        None if named => Workspace::Shared,
        None => Workspace::Isolated,
    }
}

fn hold_note(path: &Path) -> String {
    std::fs::read_to_string(crate::durable::store::lock_path(path))
        .ok()
        .and_then(|text| {
            text.lines()
                .next()
                .filter(|line| !line.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "another session".to_string())
}

impl<P: Provider> Loop<P> {
    pub(super) fn hold_children(&mut self) -> Result<()> {
        if self.children_lock.is_some() {
            return Ok(());
        }
        let path = self.workspace.join(".summonr").join("lead");
        let held = crate::durable::store::lock(&path, false).map_err(|error| {
            Error::Tool(format!("cannot lock this checkout for children: {error}"))
        })?;
        let Some(mut file) = held else {
            return Err(Error::Tool(format!(
                "this checkout already has a lead running children: {}",
                hold_note(&path)
            )));
        };
        let note = format!(
            "pid {} session {} image {}\n",
            std::process::id(),
            self.session_id(),
            current_image()
        );
        drop(file.set_len(0));
        drop(std::io::Write::write_all(&mut file, note.as_bytes()));
        self.children_lock = Some(file);
        Ok(())
    }
}

pub(super) fn child_tree(
    workspace: &Path,
    access: Access,
    selection: Workspace,
) -> Result<Option<Tree>> {
    if access != Access::Implement || selection == Workspace::Shared || !worktree::in_git(workspace)
    {
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
    let Some(result) = done.record.state.result_mut() else {
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
        let Some(result) = self.state.result_mut() else {
            return;
        };
        result.kept = landing.held;
        result.changed_paths_total = landing.paths.len();
        result.changed_paths = landing.paths;
    }
}

#[cfg(test)]
#[path = "spawn_tests.rs"]
mod tests;
