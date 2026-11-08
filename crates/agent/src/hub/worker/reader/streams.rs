use std::collections::HashMap;
use std::sync::{MutexGuard, PoisonError};

use super::Reader;
use crate::hub::{Cmd, Fact, Msg};
use crate::panel::{Fill, Stream};

impl Reader {
    /// Serves the stream panel commands and returns every other command for the usual path.
    pub(super) fn streamed(&self, command: Cmd) -> Option<Cmd> {
        match command {
            Cmd::OpenPanel(name) => self.open_stream(name),
            Cmd::ClosePanel(name) => drop(self.streams().remove(&name)),
            Cmd::ActPanel {
                name,
                event,
                context,
            } => return self.act_stream(name, event, context),
            other => return Some(other),
        }
        None
    }

    fn streams(&self) -> MutexGuard<'_, HashMap<String, Stream>> {
        self.streams.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn open_stream(&self, name: String) {
        if self.streams().get(&name).is_some_and(Stream::live) {
            return;
        }
        let panel = crate::panel::list(&self.workspace, self.trust)
            .into_iter()
            .find(|panel| panel.name == name && panel.kind == Fill::Stream);
        let Some(panel) = panel else {
            return self.send(Msg::Fact(Fact::Panel {
                name: name.clone(),
                result: Err(format!("no stream panel named `{name}`")),
            }));
        };
        let (ui, shown_name) = (self.ui.clone(), name.clone());
        let shown = move |result| {
            drop(ui.send(Msg::Fact(Fact::Panel {
                name: shown_name.clone(),
                result,
            })));
        };
        match Stream::start(&panel, &self.workspace, shown) {
            Ok(stream) => drop(self.streams().insert(name, stream)),
            Err(reason) => self.send(Msg::Fact(Fact::Panel {
                name,
                result: Err(reason),
            })),
        }
    }

    fn act_stream(&self, name: String, event: String, context: serde_json::Value) -> Option<Cmd> {
        let line = crate::panel::action(&name, &event, context.clone());
        let sent = self
            .streams()
            .get(&name)
            .is_some_and(|stream| stream.live() && stream.send(line));
        (!sent).then_some(Cmd::ActPanel {
            name,
            event,
            context,
        })
    }
}
