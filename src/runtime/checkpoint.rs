use super::*;
use crate::checkpoint::Checkpoints;
use crate::durable::log::{self, Line};

impl<P: Provider> Loop<P> {
    fn checkpoints(&mut self) -> Option<&Checkpoints> {
        if self.checkpoints.is_none() {
            self.checkpoints = Checkpoints::open(&self.workspace, &self.session_id());
        }
        self.checkpoints.as_ref()
    }

    pub(super) fn checkpoint(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if self.depth > 0 {
            return Ok(());
        }
        let Some(Err(error)) = self.checkpoints().map(Checkpoints::capture) else {
            return Ok(());
        };
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!("checkpoint skipped: {error}"),
            },
        )
    }

    pub(crate) fn rollback(&mut self, turn: Option<usize>) -> Result<(usize, Vec<String>)> {
        let checkpoints = self
            .checkpoints()
            .ok_or_else(|| Error::Config("rollback needs a git workspace".to_string()))?;
        let turn = turn.unwrap_or_else(|| checkpoints.count());
        Ok((turn, checkpoints.restore(turn)?))
    }

    pub(crate) fn undo(&mut self) -> Result<(Option<Vec<String>>, Option<String>)> {
        let turns = log::read(self.session_path())
            .map_err(super::session_err)?
            .iter()
            .filter(|line| matches!(line, Line::User { start: true, .. }))
            .count();
        if turns == 0 {
            return Err(Error::Config("nothing to undo".to_string()));
        }
        let files = match self.checkpoints() {
            Some(checkpoints) if checkpoints.count() > 0 => {
                Some(checkpoints.restore(checkpoints.count())?)
            }
            Some(_) | None => None,
        };
        let session = if turns > 1 {
            Some(self.fork_at(Some(turns - 2))?)
        } else {
            self.clear_history()?;
            None
        };
        Ok((files, session))
    }
}
