use tokio::sync::mpsc::UnboundedSender;

use crate::provider::UserInput;
use crate::tools::Event;
use crate::tui::{Activity, Fact, Intent, Msg, Snapshot};

type Roster = (Option<String>, Vec<String>, Vec<Option<String>>);

#[derive(Default)]
pub(super) struct ReplayState {
    busy: Option<Activity>,
    asking: Option<Event>,
    settings: Option<Snapshot>,
    workflow: Option<Roster>,
    mode: Option<crate::WorkKind>,
    queued: Option<(usize, usize)>,
    changed: Option<Vec<String>>,
    restored: Option<Vec<UserInput>>,
}

impl ReplayState {
    pub(super) fn record(&mut self, msg: &Msg, active: bool) {
        if let Msg::Fact(Fact::Busy(busy)) = msg {
            self.busy = *busy;
            self.asking = None;
        } else if let Msg::Fact(Fact::Event(event @ (Event::Approval { .. } | Event::Elicit { .. }))) = msg {
            self.asking = super::asking(msg)
                .filter(|asking| *asking)
                .map(|_| event.clone());
        } else if let Msg::Fact(Fact::Settings(snapshot)) = msg {
            self.settings = Some(snapshot.clone());
        } else if let Msg::Fact(Fact::Workflow {
            name,
            names,
            selections,
        }) = msg
        {
            self.workflow = Some((name.clone(), names.clone(), selections.clone()));
        } else if let Msg::Intent(Intent::RunMode(mode)) = msg {
            self.mode = Some(*mode);
        } else if let Msg::Fact(Fact::Queued { steering, queued }) = msg {
            self.queued = Some((*steering, *queued));
        } else if let Msg::Fact(Fact::Changed(paths)) = msg {
            self.changed = Some(paths.clone());
        } else if let Msg::Fact(Fact::Restored(inputs)) = msg
            && !active
        {
            self.restored = Some(inputs.clone());
        }
    }

    pub(super) const fn working(&self) -> bool {
        self.busy.is_some()
    }

    pub(super) fn resend(&mut self, app: &UnboundedSender<Msg>) {
        let mut out = vec![Msg::Fact(Fact::Busy(self.busy))];
        out.extend(
            self.settings
                .clone()
                .map(|snapshot| Msg::Fact(Fact::Settings(snapshot))),
        );
        out.extend(self.workflow.clone().map(|(name, names, selections)| {
            Msg::Fact(Fact::Workflow {
                name,
                names,
                selections,
            })
        }));
        out.extend(
            self.mode
                .map(|mode| Msg::Intent(Intent::RunMode(mode))),
        );
        let (steering, queued) = self.queued.unwrap_or_default();
        out.push(Msg::Fact(Fact::Queued { steering, queued }));
        out.extend(
            self.restored
                .take()
                .map(|restored| Msg::Fact(Fact::Restored(restored))),
        );
        out.push(Msg::Fact(Fact::Changed(self.changed.clone().unwrap_or_default())));
        out.extend(
            self.asking
                .clone()
                .map(|asking| Msg::Fact(Fact::Event(asking))),
        );
        for msg in out {
            drop(app.send(msg));
        }
    }
}
