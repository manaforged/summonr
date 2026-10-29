use super::state::set_mail_reply;
use super::*;

impl Worker {
    pub(super) async fn dispatch(&mut self, command: Cmd) -> bool {
        match command {
            Cmd::Branch { session, turn } => return self.branch(&session, turn),
            Cmd::Command(line) => return self.command_line(line).await,
            Cmd::Quit => {
                if let Err(error) = self.loop_.abort_note("shutdown") {
                    self.fail(&error.to_string());
                }
                return false;
            }
            Cmd::Mail => return self.mail().await,
            Cmd::Submit(input) => return self.submit(input).await,
            Cmd::Login(name) => return self.login(&name).await,
            Cmd::SetKey(name, key) => return self.set_key(&name, &key).await,
            Cmd::Cancel | Cmd::Open(_) | Cmd::SteerQueued | Cmd::Answer(..) => {}
            Cmd::Replay => super::history::replay_transcript(
                self.loop_.session_path(),
                Some(self.loop_.session_id()),
                Some(self.loop_.unreported_tokens()),
                &self.ui,
            ),
            Cmd::Dequeue => self.recover(),
            Cmd::Export(target) => self.export(target),
            Cmd::Logout(name) => self.logout(&name).await,
            Cmd::Steer(text) | Cmd::Queue(text) => {
                self.queue.followups.push_back(text);
                self.queue.announce(&self.ui);
            }
            Cmd::Send(to, text) => turn::mail(&self.ui, &self.loop_.session_id(), &to, &text),
            Cmd::Tick => self.tick(),
            Cmd::LoginAnswer(_) => {
                self.system("the sign-in already ended; the pasted code was not sent")
            }
            Cmd::SetEffort(effort) => {
                self.dial = effort;
                self.loop_.choose_effort(effort);
                self.loop_.set_effort(effort);
                self.send_settings();
            }
            Cmd::SetVerify(on) => self.set_verify(on),
            Cmd::ToggleApproval => self.toggle_approval(),
            Cmd::SetEndpoint(name) => self.selection(Some(&name), None).await,
            Cmd::SetModel(model) => self.selection(None, Some(&model)).await,
            Cmd::SelectModel(endpoint, model) => {
                self.selection(Some(&endpoint), Some(&model)).await
            }
            Cmd::SetMail(on) => set_mail_reply(&mut self.auto_reply, &mut self.pending_mail, on),
            Cmd::SetModule(id, on) => self.set_module(id, on),
            Cmd::AssignModel(role, endpoint, model) => {
                self.assign_model(role, &endpoint, Some(&model))
            }
            Cmd::ClearRole(role) => self.clear_role(role),
            Cmd::SaveWorkflow(name) => self.save_workflow(&name),
            Cmd::ListWorkflows => self.send_workflow(),
            Cmd::Merge(path) => self.merge(&path),
            Cmd::Discard(path) => self.discard(&path),
            command => self.reader.run(command).await,
        }
        true
    }

    fn merge(&mut self, path: &std::path::Path) {
        match self.tree_at(path).and_then(|tree| {
            crate::worktree::finish(&tree, "merge from the worktrees pane", &[]).map(|_| tree)
        }) {
            Ok(tree) => self.system(&format!("merged {} into {}", tree.branch, tree.base)),
            Err(error) => self.system(&format!("merge failed: {error}")),
        }
        self.reader.trees();
    }

    fn discard(&mut self, path: &std::path::Path) {
        match self
            .tree_at(path)
            .and_then(|tree| crate::worktree::discard(&tree).map(|()| tree))
        {
            Ok(tree) => self.system(&format!("removed worktree {}", tree.branch)),
            Err(error) => self.system(&format!("worktree kept: {error}")),
        }
        self.reader.trees();
    }

    fn toggle_approval(&mut self) {
        if self.loop_.is_strict() {
            self.system("strict policy rejects risky commands instead of asking");
        } else {
            let before = self.loop_.is_approval();
            let on = self.loop_.toggle_approval();
            let note = match (on, before == on) {
                (true, true) => "approval mode stays on: configured policy requires it",
                (true, false) => "approval mode: risky commands ask before running",
                (false, _) => {
                    "auto approval: destructive commands and policy asks still require approval"
                }
            };
            drop(self.ui.send(Msg::Approval(note.to_string())));
        }
        self.send_settings();
    }

    async fn command_line(&mut self, line: String) -> bool {
        let running = match self.command(&line).await {
            Some(running) => running,
            None => self.submit(crate::provider::UserInput::from(line)).await,
        };
        self.send_settings();
        running
    }
}
