use super::*;

impl SessionDriver {
    pub(crate) fn start_child(&mut self, record: crate::child::ChildRecord) -> Result<()> {
        if !matches!(record.state, crate::child::ChildState::Running) {
            return Err(Error::Config(
                "a child must start in the running state".to_string(),
            ));
        }
        let workspace = record.workspace == self.workspace
            || record.access == crate::child::Access::Implement
                && crate::worktree::is_delta_of(&self.workspace, &record.workspace);
        if record.version != 1
            || record.parent != self.id()
            || record.depth != self.depth().saturating_add(1)
            || !workspace
            || record.assignment.trim().is_empty()
            || record.model.trim().is_empty()
        {
            return Err(Error::Config(
                "child identity does not match the parent session".to_string(),
            ));
        }
        self.append(Line::Child { record })
    }

    pub(crate) fn finish_child(&mut self, record: crate::child::ChildRecord) -> Result<()> {
        if !record.state.terminal() {
            return Err(Error::Config("a child result must be terminal".to_string()));
        }
        match self.state.children.get(&record.id) {
            Some(child) if !child.state.terminal() && child.same_child(&record) => {
                self.append(Line::Child { record })
            }
            Some(child) if !child.same_child(&record) => Err(Error::Config(format!(
                "child {} changed identity before settlement",
                record.id
            ))),
            Some(_) => Err(Error::Config(format!(
                "child {} is already terminal",
                record.id
            ))),
            None => Err(Error::Config(format!(
                "child {} was not started by this session",
                record.id
            ))),
        }
    }

    pub(crate) fn child(&self, id: SessionId) -> Option<crate::child::ChildRecord> {
        self.state.children.get(&id).cloned()
    }

    pub(crate) fn interrupt_children(
        &mut self,
        reason: &str,
        scope: crate::child::ChildScope,
    ) -> Result<()> {
        let records = self
            .state
            .children
            .values()
            .filter(|child| !child.state.terminal())
            .filter(|child| scope == crate::child::ChildScope::All || !child.background)
            .map(|child| child.interrupt(reason))
            .collect::<Vec<_>>();
        for record in records {
            self.append(Line::Child { record })?;
        }
        Ok(())
    }
}
