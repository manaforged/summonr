use super::{Msg, Worker};
use crate::hub::Fact;
use crate::tools::Event;
use summonr_core::error::{Error, ErrorKind};

impl Worker {
    pub(super) async fn send_diff(&mut self, announce: bool) {
        let workspace = self.loop_.workspace().to_path_buf();
        let stats = tokio::task::spawn_blocking(move || crate::diff::stats(&workspace)).await;
        let diff = match stats
            .map_err(|error| Error::caused(ErrorKind::Tool, error))
            .and_then(|diff| diff)
        {
            Ok(diff) => diff,
            Err(error) => {
                self.last_diff = None;
                drop(
                    self.ui
                        .send(Msg::Fact(Fact::DiffFailed(error.to_string(), None))),
                );
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
            drop(
                self.ui
                    .send(Msg::Fact(Fact::Edited(crate::diff::edited(calls)))),
            );
            if self.last_diff.as_ref() == Some(&diff) {
                return;
            }
        }
        self.last_diff = Some(diff.clone());
        drop(self.ui.send(Msg::Fact(Fact::Diff(diff, None))));
    }

    pub(super) fn tree_at(
        &self,
        path: &std::path::Path,
    ) -> summonr_core::error::Result<crate::worktree::Tree> {
        crate::worktree::list(self.loop_.workspace())?
            .into_iter()
            .find(|tree| tree.path == path)
            .ok_or_else(|| {
                summonr_core::error::Error::Config(format!("no worktree at {}", path.display()))
            })
    }
}
