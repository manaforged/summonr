use super::*;

impl Worker {
    pub(super) async fn dispatch(&mut self, command: Cmd) -> bool {
        match command {
            command @ (Cmd::Branch { .. }
            | Cmd::Command(_)
            | Cmd::Quit
            | Cmd::Cancel
            | Cmd::Dequeue
            | Cmd::Export(_)
            | Cmd::Mail
            | Cmd::Submit(_)) => self.lifecycle(command).await,
            command @ (Cmd::Login(_) | Cmd::SetKey(_, _) | Cmd::Logout(_)) => {
                self.auth(command).await
            }
            command @ (Cmd::Steer(_)
            | Cmd::Queue(_)
            | Cmd::Send(..)
            | Cmd::Tick
            | Cmd::LoginAnswer(_)) => self.traffic(command),
            command @ (Cmd::SetRunMode(_)
            | Cmd::SetEffort(_)
            | Cmd::SetVerify(_)
            | Cmd::SetEndpoint(_)
            | Cmd::SetModel(_)
            | Cmd::SetSeat(..)
            | Cmd::SetMail(_)
            | Cmd::SetModule(..)) => self.settings(command).await,
            command @ (Cmd::SeatRole(..)
            | Cmd::ClearRole(_)
            | Cmd::SaveWorkflow(_)
            | Cmd::ListWorkflows) => self.roster(command),
            command @ (Cmd::Merge(_) | Cmd::Discard(_)) => self.worktree(command),
            command @ (Cmd::ListModels { .. }
            | Cmd::Pin(..)
            | Cmd::SaveDefault(_)
            | Cmd::ListSessions
            | Cmd::ListUsage
            | Cmd::DeleteSession(_)
            | Cmd::ListPrompts(_)
            | Cmd::ListFiles
            | Cmd::Jobs
            | Cmd::Mcp
            | Cmd::KillJob(_)
            | Cmd::Clipboard
            | Cmd::Diff(_)
            | Cmd::Patch(..)
            | Cmd::Review { .. }
            | Cmd::Trees
            | Cmd::Offers
            | Cmd::View(..)
            | Cmd::Save(..)) => {
                self.reader.run(command).await;
                true
            }
        }
    }

    async fn lifecycle(&mut self, command: Cmd) -> bool {
        match command {
            Cmd::Branch { session, turn } => self.branch(&session, turn),
            Cmd::Command(line) => {
                let running = self.command(&line).await.unwrap_or(true);
                self.send_settings();
                running
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
            Cmd::Mail => self.mail().await,
            Cmd::Submit(input) => self.submit(input).await,
            _command => unreachable!("dispatch routes only lifecycle commands"),
        }
    }

    async fn auth(&mut self, command: Cmd) -> bool {
        match command {
            Cmd::Login(name) => self.login(&name).await,
            Cmd::SetKey(name, key) => self.set_key(&name, &key).await,
            Cmd::Logout(name) => {
                self.logout(&name);
                true
            }
            _command => unreachable!("dispatch routes only auth commands"),
        }
    }

    fn traffic(&mut self, command: Cmd) -> bool {
        match command {
            Cmd::Steer(text) | Cmd::Queue(text) => {
                self.followups.push_back(text);
                announce(&self.ui, &self.steering, &self.followups);
            }
            Cmd::Send(to, text) => {
                turn::mail(&self.ui, &self.loop_.session_id(), &to, &text);
            }
            Cmd::Tick => self.tick(),
            Cmd::LoginAnswer(_) => {
                self.system("the sign-in already ended; the pasted code was not sent")
            }
            _command => unreachable!("dispatch routes only traffic commands"),
        }
        true
    }

    async fn settings(&mut self, command: Cmd) -> bool {
        match command {
            Cmd::SetRunMode(mode) => set_run_mode(&mut self.loop_, &self.ui, mode),
            Cmd::SetEffort(effort) => {
                self.dial = effort;
                self.loop_.set_effort(effort);
                self.remember_seat();
                self.send_settings();
            }
            Cmd::SetVerify(on) => self.set_verify(on),
            Cmd::SetEndpoint(name) => self.set_endpoint(&name).await,
            Cmd::SetModel(model) => self.set_model(&model).await,
            Cmd::SetSeat(endpoint, model) => self.set_seat(&endpoint, &model).await,
            Cmd::SetMail(on) => self.set_mail_reply(on),
            Cmd::SetModule(id, on) => self.set_module(id, on),
            _command => unreachable!("dispatch routes only settings commands"),
        }
        true
    }

    fn roster(&mut self, command: Cmd) -> bool {
        match command {
            Cmd::SeatRole(role, endpoint, model) => self.seat_role(role, &endpoint, Some(&model)),
            Cmd::ClearRole(role) => self.clear_role(role),
            Cmd::SaveWorkflow(name) => self.save_workflow(&name),
            Cmd::ListWorkflows => self.send_workflow(),
            _command => unreachable!("dispatch routes only roster commands"),
        }
        true
    }

    fn worktree(&mut self, command: Cmd) -> bool {
        match command {
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
            }
            _command => unreachable!("dispatch routes only worktree commands"),
        }
        self.reader.trees();
        true
    }
}
