use super::*;
use std::collections::BTreeMap;

impl SessionDriver {
    pub(crate) fn defer(&mut self, note: &str, jobs: Vec<String>) -> Result<bool> {
        if self.turn.is_none() {
            return Err(no_turn());
        }
        let deferred = self
            .work
            .as_mut()
            .ok_or_else(no_work)?
            .defer(note.to_string(), jobs)?;
        if deferred {
            self.checkpoint()?;
            self.close()?;
        }
        Ok(deferred)
    }

    pub(crate) fn jobs(&self) -> Vec<String> {
        self.active()
            .map(|work| work.jobs.clone())
            .unwrap_or_default()
    }

    pub(crate) fn progress(&self) -> Option<WorkProgress> {
        self.work.as_ref().map(Work::progress)
    }

    pub(crate) fn work_progress(&self) -> Result<WorkProgress> {
        self.progress().ok_or_else(no_work)
    }

    pub(crate) fn apply_todos(&mut self, todos: Vec<summonr_types::Todo>) -> Result<bool> {
        let Some(work) = self.work.as_mut().filter(|work| !work.terminal()) else {
            return Ok(false);
        };
        work.apply_todos(todos);
        self.checkpoint()?;
        Ok(true)
    }

    pub(crate) fn abandon_work(&mut self, reason: &str) -> Result<()> {
        self.work.as_mut().ok_or_else(no_work)?.abandon(reason);
        self.checkpoint()
    }

    pub(crate) fn objective(&self) -> Result<String> {
        Ok(self.work.as_ref().ok_or_else(no_work)?.objective.clone())
    }

    pub(crate) fn needs_scout(&self) -> bool {
        self.active().is_some_and(|work| {
            work.brief.is_none()
                && match work.policy {
                    WorkKind::Steps | WorkKind::Goal => true,
                    WorkKind::Single => false,
                }
        })
    }

    pub(crate) fn set_brief(&mut self, brief: &str) -> Result<()> {
        self.work.as_mut().ok_or_else(no_work)?.brief = Some(brief.to_string());
        self.checkpoint()
    }

    pub(crate) fn goal_objective(&self) -> Result<Option<String>> {
        let work = self.work.as_ref().ok_or_else(no_work)?;
        Ok(matches!(work.policy, WorkKind::Goal).then(|| work.objective.clone()))
    }

    pub(crate) fn goal_active(&self) -> bool {
        self.active()
            .is_some_and(|work| matches!(work.policy, WorkKind::Goal))
    }

    pub(crate) fn adopt_checks(
        &mut self,
        commands: &[String],
        protected: BTreeMap<String, Option<String>>,
    ) -> Result<bool> {
        let Some(work) = self.work.as_mut().filter(|work| !work.terminal()) else {
            return Ok(false);
        };
        if !work.adopt_checks(commands, protected) {
            return Ok(false);
        }
        self.checkpoint()?;
        Ok(true)
    }

    pub(crate) fn protected(&self) -> BTreeMap<String, Option<String>> {
        self.active()
            .map(|work| work.protected.clone())
            .unwrap_or_default()
    }

    pub(crate) fn completion_checks(&self) -> Result<Vec<Check>> {
        Ok(self.work.as_ref().ok_or_else(no_work)?.completion_checks())
    }

    pub(crate) fn prepare_external_work(
        &mut self,
        text: &str,
        policy: WorkKind,
        commands: &[String],
    ) -> Result<()> {
        if policy == WorkKind::Single {
            return Err(Error::Config(
                "external Work requires Steps or Goal policy".to_string(),
            ));
        }
        self.adopt_external_work(text, policy)?;
        let work = self.work.as_mut().ok_or_else(no_work)?;
        if work.plan.is_none() {
            work.select(text, commands)?;
        }
        work.adopt_checks(commands, BTreeMap::new());
        let expected = crate::durable::work::checks(text, policy, commands)?;
        let kept = policy == WorkKind::Goal && commands.is_empty();
        if work.plan.as_deref() != Some(text) || (!kept && work.checks != expected) {
            return Err(Error::Config(
                "the plan or completion checks changed while Work was active".to_string(),
            ));
        }
        self.resume_external_work()
    }

    fn adopt_external_work(&mut self, text: &str, policy: WorkKind) -> Result<()> {
        match self.active() {
            Some(work)
                if std::mem::discriminant(&work.policy) != std::mem::discriminant(&policy) =>
            {
                return Err(Error::Config(
                    "another Work is active; /goal clear or /steps clear ends it".to_string(),
                ));
            }
            Some(work) if work.objective != text => {
                return Err(Error::Config(format!(
                    "`{}` is still active; /goal clear or /steps clear ends it",
                    work.objective
                )));
            }
            Some(_) => {}
            None => self.set_work(text, policy)?,
        }
        Ok(())
    }

    fn resume_external_work(&mut self) -> Result<()> {
        let stage = self.work.as_ref().ok_or_else(no_work)?.stage();
        match stage {
            Some(Stage::Plan) => {
                self.checkpoint()?;
                self.execute()
            }
            Some(Stage::Execute) => self.checkpoint(),
            Some(Stage::Verify) => Err(Error::Config(
                "Work is still in Verify after an interrupted turn".to_string(),
            )),
            None => Err(no_work()),
        }
    }

    pub(crate) fn prepare_turn_work(&mut self, objective: &str, commands: &[String]) -> Result<()> {
        if self.active().is_none() {
            let todos = self
                .work
                .as_ref()
                .map(|work| work.todos.clone())
                .unwrap_or_default();
            let mut work = Work::new(objective, WorkKind::Single);
            work.select(objective, commands)?;
            work.apply_todos(todos);
            self.work = Some(work);
            self.checkpoint()?;
        }
        self.sync_turn_checks(commands)?;
        self.advance_turn_work(objective, commands)
    }

    fn sync_turn_checks(&mut self, commands: &[String]) -> Result<()> {
        let work = self.work.as_mut().ok_or_else(no_work)?;
        if work.policy == WorkKind::Single
            && work
                .checks
                .iter()
                .map(|check| check.command.as_str())
                .ne(commands.iter().map(String::as_str))
        {
            let plan = work.plan.clone().unwrap_or_else(|| work.objective.clone());
            work.select(&plan, commands)?;
            self.checkpoint()?;
        }
        Ok(())
    }

    fn advance_turn_work(&mut self, objective: &str, commands: &[String]) -> Result<()> {
        let work = self.work.as_ref().ok_or_else(no_work)?;
        if work.policy == WorkKind::Single && work.objective != objective {
            return self.replace_single(objective, commands);
        }
        match work.stage() {
            Some(Stage::Plan) if work.policy == WorkKind::Single && work.plan.is_none() => {
                self.replan_turn_work(objective, commands)
            }
            Some(Stage::Plan) => self.execute(),
            Some(Stage::Execute) => Ok(()),
            Some(Stage::Verify) => Err(Error::Config(
                "Work is still in Verify after an interrupted turn".to_string(),
            )),
            None => Err(no_work()),
        }
    }

    fn replace_single(&mut self, objective: &str, commands: &[String]) -> Result<()> {
        self.set_work(objective, WorkKind::Single)?;
        self.work
            .as_mut()
            .ok_or_else(no_work)?
            .select(objective, commands)?;
        self.execute()
    }

    fn replan_turn_work(&mut self, objective: &str, commands: &[String]) -> Result<()> {
        if let Some(work) = self.work.as_mut() {
            work.abandon("the plan turn selected no plan");
        }
        self.set_work(objective, WorkKind::Single)?;
        self.work
            .as_mut()
            .ok_or_else(no_work)?
            .select(objective, commands)?;
        self.execute()
    }

    pub(crate) fn record_check(&mut self, passed: bool) -> Result<()> {
        self.work.as_mut().ok_or_else(no_work)?.record_check(passed);
        self.checkpoint()
    }

    pub(crate) fn measured(&self) -> Result<bool> {
        Ok(!self
            .work
            .as_ref()
            .ok_or_else(no_work)?
            .measurements
            .is_empty())
    }

    pub(crate) fn best_measurement(&self) -> Result<Option<f64>> {
        Ok(self.work.as_ref().ok_or_else(no_work)?.best())
    }

    pub(crate) fn record_measurement(&mut self, value: Option<String>, kept: bool) -> Result<()> {
        self.work.as_mut().ok_or_else(no_work)?.measure(value, kept);
        self.checkpoint()
    }

    pub(super) fn active(&self) -> Option<&Work> {
        self.work.as_ref().filter(|work| !work.terminal())
    }

    pub(super) fn set_work(&mut self, objective: &str, policy: WorkKind) -> Result<()> {
        self.work = Some(Work::new(objective, policy));
        self.checkpoint()
    }
}
