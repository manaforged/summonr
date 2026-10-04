use super::*;

impl SessionDriver {
    pub(crate) fn start_child(
        &mut self,
        record: crate::child::ChildRecord,
    ) -> Result<(), DriverError> {
        if !matches!(record.state, crate::child::ChildState::Running) {
            return Err(DriverError::State(
                "a child must start in the running state".to_string(),
            ));
        }
        let workspace = record.workspace == self.workspace
            || record.access == crate::child::Access::Implement
                && crate::worktree::same_repository(&self.workspace, &record.workspace);
        if record.version != 1
            || record.parent != self.id()
            || record.depth != self.depth().saturating_add(1)
            || !workspace
            || record.assignment.trim().is_empty()
            || record.model.trim().is_empty()
        {
            return Err(DriverError::State(
                "child identity does not match the parent session".to_string(),
            ));
        }
        self.append(Line::Child { record })
    }

    pub(crate) fn finish_child(
        &mut self,
        record: crate::child::ChildRecord,
    ) -> Result<(), DriverError> {
        if !record.state.terminal() {
            return Err(DriverError::State(
                "a child result must be terminal".to_string(),
            ));
        }
        match self.state.children.get(&record.id) {
            Some(child) if !child.state.terminal() && child.same_child(&record) => {
                self.append(Line::Child { record })
            }
            Some(child) if !child.same_child(&record) => Err(DriverError::State(format!(
                "child {} changed identity before settlement",
                record.id
            ))),
            Some(_) => Err(DriverError::State(format!(
                "child {} is already terminal",
                record.id
            ))),
            None => Err(DriverError::State(format!(
                "child {} was not started by this session",
                record.id
            ))),
        }
    }

    pub(crate) fn child(&self, id: SessionId) -> Option<crate::child::ChildRecord> {
        self.state.children.get(&id).cloned()
    }

    pub(crate) fn interrupt_children(&mut self, reason: &str) -> Result<(), DriverError> {
        let records = self
            .state
            .children
            .values()
            .filter(|child| !child.state.terminal())
            .map(|child| child.interrupt(reason))
            .collect::<Vec<_>>();
        for record in records {
            self.append(Line::Child { record })?;
        }
        Ok(())
    }
}
