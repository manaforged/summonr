use std::sync::Arc;

use tokio::sync::mpsc::UnboundedSender;

use crate::hub::{Activity, Fact, Intent, Msg, Snapshot};
use crate::provider::UserInput;
use crate::tools::Event;

type Roster = (Option<String>, Vec<String>, Vec<Option<String>>);

#[derive(Default)]
pub(super) struct Replay {
    busy: Option<Activity>,
    asking: Option<Event>,
    settings: Option<Arc<Snapshot>>,
    workflow: Option<Roster>,
    mode: Option<crate::WorkKind>,
    queued: Option<(usize, usize)>,
    pending: Arc<[(bool, String)]>,
    changed: Option<Arc<[String]>>,
    restored: Option<Vec<UserInput>>,
}

impl Replay {
    pub(super) fn record(&mut self, msg: &Msg, active: bool) {
        match msg {
            Msg::Fact(Fact::Busy(busy)) => {
                self.busy = *busy;
                self.asking = None;
            }
            Msg::Fact(Fact::Event(event @ (Event::Approval { .. } | Event::Elicit { .. }))) => {
                self.asking = super::asking(event)
                    .filter(|asking| *asking)
                    .map(|_| event.clone());
            }
            Msg::Fact(Fact::Settings(snapshot)) => self.settings = Some(Arc::clone(snapshot)),
            Msg::Fact(Fact::Workflow {
                name,
                names,
                selections,
            }) => self.workflow = Some((name.clone(), names.clone(), selections.clone())),
            Msg::Intent(Intent::RunMode(mode)) => self.mode = Some(*mode),
            Msg::Fact(Fact::Queued { steering, queued }) => {
                self.queued = Some((*steering, *queued));
            }
            Msg::Fact(Fact::Pending(rows)) => self.pending = Arc::clone(rows),
            Msg::Fact(Fact::Changed(paths)) => self.changed = Some(Arc::clone(paths)),
            Msg::Fact(Fact::Restored(inputs)) if !active => self.restored = Some(inputs.clone()),
            _ => {}
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
        out.extend(self.mode.map(|mode| Msg::Intent(Intent::RunMode(mode))));
        let (steering, queued) = self.queued.unwrap_or_default();
        out.push(Msg::Fact(Fact::Queued { steering, queued }));
        out.push(Msg::Fact(Fact::Pending(Arc::clone(&self.pending))));
        out.extend(
            self.restored
                .take()
                .map(|restored| Msg::Fact(Fact::Restored(restored))),
        );
        out.push(Msg::Fact(Fact::Changed(
            self.changed.clone().unwrap_or_default(),
        )));
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
