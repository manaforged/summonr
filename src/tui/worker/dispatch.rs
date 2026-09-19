use super::*;

impl Worker {
    pub(super) async fn dispatch(&mut self, command: Cmd) -> bool {
        match command {
            Cmd::Branch { session, turn } => self.branch(&session, turn),
            Cmd::Command(line) => {
                let running = self.command(&line).await.unwrap_or(true);
                self.send_settings();
                running
            }
            Cmd::Login(name) => self.login(&name).await,
            Cmd::SetKey(name, key) => {
                self.set_key(&name, &key).await;
                true
            }
            Cmd::Quit => {
                if let Err(error) = self.loop_.abort_note("shutdown") {
                    self.fail(&error.to_string());
                }
                false
            }
            Cmd::Cancel => true,
            Cmd::Dequeue => {
                self.recover();
                true
            }
            Cmd::Export(target) => {
                self.export(target);
                true
            }
            Cmd::Steer(text) | Cmd::Queue(text) => {
                self.followups.push_back(text);
                announce(&self.ui, &self.steering, &self.followups);
                true
            }
            Cmd::Send(to, text) => {
                turn::mail(&self.ui, &self.loop_.session_id(), &to, &text);
                true
            }
            Cmd::Tick => {
                self.tick();
                true
            }
            Cmd::SetRunMode(mode) => {
                set_run_mode(&mut self.loop_, &self.ui, mode);
                true
            }
            Cmd::SetEffort(effort) => {
                self.dial = effort;
                self.loop_.set_effort(effort);
                self.send_settings();
                true
            }
            Cmd::SetVerify(on) => {
                self.set_verify(on);
                true
            }
            Cmd::SetEndpoint(name) => {
                self.set_endpoint(&name).await;
                true
            }
            Cmd::SetModel(model) => {
                self.set_model(&model).await;
                true
            }
            Cmd::SetSeat(endpoint, model) => {
                self.set_seat(&endpoint, &model).await;
                true
            }
            command @ (Cmd::ListModels { .. }
            | Cmd::ListSessions
            | Cmd::ListUsage
            | Cmd::DeleteSession(_)
            | Cmd::ListPrompts(_)
            | Cmd::ListFiles
            | Cmd::Diff(_)
            | Cmd::Patch(..)
            | Cmd::Trees
            | Cmd::View(..)
            | Cmd::Save(..)) => {
                self.reader.run(command).await;
                true
            }
            Cmd::Merge(path) => {
                match self.tree_at(&path).and_then(|tree| {
                    crate::worktree::finish(&tree, "merge from the worktrees pane", &[])
                        .map(|_| tree)
                }) {
                    Ok(tree) => drop(self.ui.send(Msg::System(format!(
                        "merged {} into {}",
                        tree.branch, tree.base
                    )))),
                    Err(error) => drop(self.ui.send(Msg::System(format!("merge failed: {error}")))),
                }
                self.reader.trees();
                true
            }
            Cmd::Discard(path) => {
                match self
                    .tree_at(&path)
                    .and_then(|tree| crate::worktree::discard(&tree).map(|()| tree))
                {
                    Ok(tree) => drop(
                        self.ui
                            .send(Msg::System(format!("removed worktree {}", tree.branch))),
                    ),
                    Err(error) => {
                        drop(self.ui.send(Msg::System(format!("worktree kept: {error}"))))
                    }
                }
                self.reader.trees();
                true
            }
            Cmd::SeatRole(role, endpoint, model) => {
                self.seat_role(role, &endpoint, Some(&model));
                true
            }
            Cmd::ClearRole(role) => {
                self.clear_role(role);
                true
            }
            Cmd::SaveWorkflow(name) => {
                self.save_workflow(&name);
                true
            }
            Cmd::ListWorkflows => {
                self.send_workflow();
                true
            }
            Cmd::SetBus(on) => {
                self.set_bus(on);
                true
            }
            Cmd::SetModule(name, on) => {
                if let Err(error) = crate::modules::set(&name, on) {
                    self.fail(&error.to_string());
                    return true;
                }
                match crate::modules::Switches::load(self.loop_.workspace()) {
                    Ok(switches) => self.loop_.set_modules(&switches),
                    Err(error) => self.fail(&error.to_string()),
                }
                true
            }
            Cmd::Mail => self.mail().await,
            Cmd::Submit(input) => self.submit(input).await,
        }
    }
}
