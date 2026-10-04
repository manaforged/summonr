use tokio::sync::mpsc::UnboundedSender;

use crate::provider::UserInput;
use crate::tools::Event;
use crate::tui::{Activity, Msg, Snapshot};

type Roster = (Option<String>, Vec<String>, Vec<Option<String>>);

#[derive(Default)]
pub(super) struct Sticky {
    busy: Option<Activity>,
    asking: Option<Event>,
    settings: Option<Snapshot>,
    workflow: Option<Roster>,
    mode: Option<crate::RunMode>,
    queued: Option<(usize, usize)>,
    changed: Option<Vec<String>>,
    restored: Option<Vec<UserInput>>,
}

impl Sticky {
    pub(super) fn record(&mut self, msg: &Msg, active: bool) {
        if let Msg::Busy(busy) = msg {
            self.busy = *busy;
            self.asking = None;
        } else if let Msg::Event(event @ (Event::Approval { .. } | Event::Elicit { .. })) = msg {
            self.asking = super::asking(msg)
                .filter(|asking| *asking)
                .map(|_| event.clone());
        } else if let Msg::Settings(snapshot) = msg {
            self.settings = Some(snapshot.clone());
        } else if let Msg::Workflow { name, names, seats } = msg {
            self.workflow = Some((name.clone(), names.clone(), seats.clone()));
        } else if let Msg::RunMode(mode) = msg {
            self.mode = Some(*mode);
        } else if let Msg::Queued { steering, queued } = msg {
            self.queued = Some((*steering, *queued));
        } else if let Msg::Changed(paths) = msg {
            self.changed = Some(paths.clone());
        } else if let Msg::Restored(inputs) = msg
            && !active
        {
            self.restored = Some(inputs.clone());
        }
    }

    pub(super) const fn working(&self) -> bool {
        self.busy.is_some()
    }

    pub(super) fn resend(&mut self, app: &UnboundedSender<Msg>) {
        let mut out = vec![Msg::Busy(self.busy)];
        out.extend(self.settings.clone().map(Msg::Settings));
        out.extend(
            self.workflow
                .clone()
                .map(|(name, names, seats)| Msg::Workflow { name, names, seats }),
        );
        out.extend(self.mode.map(Msg::RunMode));
        let (steering, queued) = self.queued.unwrap_or_default();
        out.push(Msg::Queued { steering, queued });
        out.extend(self.restored.take().map(Msg::Restored));
        out.push(Msg::Changed(self.changed.clone().unwrap_or_default()));
        out.extend(self.asking.clone().map(Msg::Event));
        for msg in out {
            drop(app.send(msg));
        }
    }
}
