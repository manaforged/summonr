use super::{Msg, Worker};
use crate::tools::Event;

impl Worker {
    pub(super) fn send_diff(&mut self, announce: bool) {
        let diff = match crate::diff::stats(self.loop_.workspace()) {
            Ok(diff) => diff,
            Err(error) => {
                self.last_diff = None;
                drop(self.ui.send(Msg::DiffFailed(error.to_string())));
                return;
            }
        };
        if announce {
            let calls = self.last_turn.iter().flat_map(|turn| {
                turn.events.iter().filter_map(|event| match event {
                    Event::ToolCall {
                        name, arguments, ..
                    } => Some((name.as_str(), arguments.as_str())),
                    _ => None,
                })
            });
            drop(self.ui.send(Msg::Edited(crate::diff::edited(calls))));
            if self.last_diff.as_ref() == Some(&diff) {
                return;
            }
        }
        self.last_diff = Some(diff.clone());
        drop(self.ui.send(Msg::Diff(diff)));
    }

    pub(super) fn tree_at(
        &self,
        path: &std::path::Path,
    ) -> crate::error::Result<crate::worktree::Tree> {
        crate::worktree::list(self.loop_.workspace())?
            .into_iter()
            .find(|tree| tree.path == path)
            .ok_or_else(|| {
                crate::error::Error::Config(format!("no worktree at {}", path.display()))
            })
    }
}
