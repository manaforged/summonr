use super::*;

impl<P: Provider> Loop<P> {
    pub(crate) fn active_goal(&self) -> Result<Option<(String, crate::WorkProgress)>> {
        let Some(objective) = self.session.goal_objective()? else {
            return Ok(None);
        };
        let progress = self.work_progress()?;
        Ok((!progress.terminal()).then_some((objective, progress)))
    }

    #[must_use]
    pub(crate) fn goal_status(&self) -> Option<String> {
        let (objective, progress) = self.active_goal().ok().flatten()?;
        let round = progress.rounds;
        let mut text = format!(
            "goal active: {}\nround {round} · {} tokens · {}",
            crate::display::ellipsize(objective.lines().next().unwrap_or_default(), 80),
            self.spent_tokens(),
            progress.phase,
        );
        let gates = self.verify_commands();
        if !gates.is_empty() {
            text.push_str("\ngates: ");
            text.push_str(&gates.join(" then "));
        }
        if let Some(reason) = &progress.reason {
            text.push_str("\nlast: ");
            text.push_str(&crate::display::ellipsize(
                reason.lines().next().unwrap_or_default(),
                160,
            ));
        }
        Some(text)
    }

    pub(crate) fn abandon_work(&mut self, reason: &str) -> Result<()> {
        self.session.abandon_work(reason)
    }

    pub(crate) fn fork_live(&mut self) -> Result<String> {
        self.fork_at(None)
    }

    pub(crate) fn fork_at(&mut self, turn: Option<usize>) -> Result<String> {
        if !self.spawned.is_empty() {
            return Err(Error::Config(
                "Collect or stop subagents before branching".to_string(),
            ));
        }
        let path = self.session.fork(turn)?;
        let child = resume_driver(&self.workspace, &path)?;
        let id = child.id().to_string();
        self.adopt(child)?;
        Ok(id)
    }

    pub(crate) fn adopt(&mut self, session: SessionDriver) -> Result<()> {
        self.forget_spawned(crate::child::ChildScope::All, "the session changed")?;
        self.session = session;
        self.tools.set_session(&self.session.id().to_string());
        self.rewatch_jobs(None);
        self.mailbox = None;
        self.join_mail();
        self.schema = None;
        self.tools.forget_reads();
        self.tools.forget_approval();
        self.outputs_cleared = false;
        self.herdr.retarget(&self.session.id().to_string());
        Ok(())
    }

    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(crate) async fn judge(&mut self, system: &str, user: &str) -> Result<(String, Usage)> {
        let messages = [
            Message::system(system.to_string()),
            Message::user(user.to_string()),
        ];
        match self
            .side_call(workflow::Role::Judge, &messages, None)
            .await?
        {
            Ok(reply) => {
                let usage = reply.message.usage.clone();
                let text = reply.message.content.unwrap_or_default();
                if let Some(reason) = self.round_abort() {
                    return Err(Error::Config(reason));
                }
                Ok((text, usage))
            }
            Err(missed) => Err(missed.error.error()),
        }
    }

    /// Names the session in a few words from its first request and reply.
    pub(crate) async fn title(&mut self, request: &str, reply: &str) -> Result<String> {
        let start: String = reply.chars().take(TITLE_REPLY_CHARS).collect();
        let messages = [
            Message::system(TITLE_SYSTEM.to_string()),
            Message::user(format!("Request:\n{request}\n\nReply (start):\n{start}")),
        ];
        match self
            .side_call(workflow::Role::Title, &messages, None)
            .await?
        {
            Ok(reply) => Ok(title_text(&reply.message.content.unwrap_or_default())),
            Err(missed) => Err(missed.error.error()),
        }
    }

    pub(crate) fn clear_history(&mut self) -> Result<()> {
        self.session.clear()?;
        self.tools.forget_reads();
        self.restyle();
        Ok(())
    }

    pub(crate) fn abort_note(&mut self, reason: &str) -> Result<()> {
        let abort = crate::tools::Abort {
            code: crate::tools::AbortCode::Interrupted,
            reason: reason.to_string(),
        };
        self.abort_with(&abort, |_| {})
    }

    pub(crate) fn abort_with(
        &mut self,
        abort: &crate::tools::Abort,
        mut emit: impl FnMut(&Event),
    ) -> Result<()> {
        let forgotten = self.forget_spawned(abort.code.child_scope(), &abort.reason);
        self.tools.forget_approval();
        self.session.abort(abort, None)?;
        if abort.code.child_scope() == crate::child::ChildScope::All {
            self.tools.stop_jobs()?;
        } else {
            self.tools.stop_detached()?;
        }
        self.watch_jobs();
        self.emit_records(&mut Vec::new(), &mut emit)?;
        self.sync_usage(&mut emit);
        forgotten
    }

    pub(crate) fn unreported_tokens(&self) -> u64 {
        self.session.usage().1
    }

    pub fn select_steps(&mut self, markdown: &str, commands: &[String]) -> Result<()> {
        self.session
            .prepare_external_work(markdown, crate::WorkKind::Steps, commands)?;
        self.schema = None;
        self.set_mode(crate::WorkKind::Steps);
        Ok(())
    }

    pub fn select_goal(&mut self, objective: &str, commands: &[String]) -> Result<()> {
        self.session
            .prepare_external_work(objective, crate::WorkKind::Goal, commands)?;
        self.schema = None;
        self.set_mode(crate::WorkKind::Goal);
        Ok(())
    }

    #[must_use]
    pub fn touched_at(path: &Path) -> Vec<String> {
        crate::durable::driver::touched_at(path)
    }

    #[must_use]
    pub fn job_at(path: &Path, workspace: &Path) -> Option<(crate::WorkKind, String)> {
        crate::durable::driver::active_at(path, workspace)
    }

    #[must_use]
    pub fn steps_at(path: &Path, workspace: &Path, markdown: &str) -> bool {
        work_at(path, workspace, crate::WorkKind::Steps, markdown)
    }

    #[must_use]
    pub fn goal_at(path: &Path, workspace: &Path, objective: &str) -> bool {
        work_at(path, workspace, crate::WorkKind::Goal, objective)
    }
}

const TITLE_SYSTEM: &str = "Name this coding session in 2 to 6 words, like a short task title. \
Use sentence case. Reply with the title only: no quotes, no punctuation at the end, no markdown.";
const TITLE_REPLY_CHARS: usize = 600;
const TITLE_WORDS: usize = 6;
const TITLE_CHARS: usize = 50;

fn title_text(reply: &str) -> String {
    let line = reply
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let words: Vec<&str> = line
        .split_whitespace()
        .map(|word| word.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | '*' | '#')))
        .filter(|word| !word.is_empty())
        .take(TITLE_WORDS)
        .collect();
    let title = words.join(" ");
    let title = title.trim_end_matches(['.', ':', ';', ',', '!']);
    crate::display::ellipsize(title, crate::display::Span::Cells(TITLE_CHARS))
}
