use super::*;
use crate::checkpoint::Checkpoints;
use crate::durable::log::{self, Line};

pub(super) type Pretask = (
    Option<(
        tokio::task::JoinHandle<String>,
        tokio::task::JoinHandle<Option<String>>,
    )>,
    String,
);

impl<P: Provider> Loop<P> {
    fn checkpoints(&mut self) -> Option<&Checkpoints> {
        if self.checkpoints.is_none() {
            self.checkpoints = Checkpoints::open(&self.workspace, &self.session_id());
        }
        self.checkpoints.as_ref()
    }

    pub(super) fn checkpoint(&mut self) -> Pretask {
        let workspace = self.workspace.clone();
        let points = (self.depth == 0).then(|| self.checkpoints().cloned());
        let capture = tokio::task::spawn_blocking(move || {
            points
                .flatten()
                .and_then(|points| points.capture().err())
                .map(|error| error.to_string())
        });
        let snapshot = tokio::task::spawn_blocking(move || crate::diff::snapshot(&workspace));
        (Some((snapshot, capture)), String::new())
    }

    pub(super) async fn settle(
        &mut self,
        pre: &mut Pretask,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<String> {
        if let Some((snapshot, capture)) = pre.0.take() {
            let (snapshot, skipped) = tokio::join!(snapshot, capture);
            pre.1 = snapshot.map_err(std::io::Error::from)?;
            if let Some(error) = skipped.map_err(std::io::Error::from)? {
                let text = format!("checkpoint skipped: {error}");
                self.push_event(events, emit, Event::Note { text })?;
            }
        }
        Ok(pre.1.clone())
    }

    pub(crate) fn rollback(&mut self, turn: Option<usize>) -> Result<(usize, Vec<String>)> {
        let checkpoints = self
            .checkpoints()
            .ok_or_else(|| Error::Config("/undo files needs a git workspace".to_string()))?;
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
