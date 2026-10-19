use std::path::PathBuf;
use std::sync::Arc;

use super::Msg;
use crate::error::Result;
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tools::Event;
use crate::workflow::Workflow;

mod run;
mod sticky;
#[cfg(test)]
mod tests;

pub(super) use run::{Started, start};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Open {
    New,
    Worktree,
    Resume(String),
}

pub type Factory =
    Arc<dyn Fn(Open, super::Ui) -> Result<(Loop<Client>, Option<Workflow>)> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Working,
    NeedsYou,
    Review,
    Failed,
    Idle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Live {
    pub(crate) session: String,
    pub(crate) title: String,
    pub(crate) workspace: PathBuf,
    pub(crate) status: Status,
    pub(crate) active: bool,
}

impl Status {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::NeedsYou => "needs you",
            Self::Review => "ready",
            Self::Failed => "failed",
            Self::Idle => "",
        }
    }
}

pub(crate) fn status(prev: Status, msg: &Msg) -> Status {
    if let Msg::Busy(activity) = msg {
        return if activity.is_some() {
            Status::Working
        } else if matches!(prev, Status::Working | Status::NeedsYou) {
            Status::Review
        } else {
            prev
        };
    }
    if let Some(asking) = asking(msg) {
        return if asking {
            Status::NeedsYou
        } else if prev == Status::NeedsYou {
            Status::Working
        } else {
            prev
        };
    }
    if matches!(msg, Msg::Failed(_)) && prev == Status::Working {
        return Status::Failed;
    }
    prev
}

pub(crate) const fn seen(status: Status) -> Status {
    if matches!(status, Status::Review | Status::Failed) {
        Status::Idle
    } else {
        status
    }
}

fn asking(msg: &Msg) -> Option<bool> {
    if let Msg::Event(Event::Approval { detail, .. }) = msg {
        return Some(detail.is_some());
    }
    if let Msg::Event(Event::Elicit { form, .. }) = msg {
        return Some(form.is_some());
    }
    None
}
