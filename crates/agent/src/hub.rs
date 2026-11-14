use std::path::PathBuf;
use std::sync::Arc;

use crate::provider::Client;
use crate::runtime::Loop;
use crate::tools::Event;
use crate::workflow::Workflow;
use summonr_core::error::Result;

mod channel;
mod message;
mod replay;
mod run;
mod worker;

pub use channel::{Activity, client};
pub use channel::{Inbox, Ui, channel};
pub use message::Info;
pub use message::{Cmd, Fact, Intent, Msg, RETRY_HINT, Round, Snapshot};
use worker::worker;
#[cfg(test)]
mod tests;

pub use run::{Started, start};

/// A second window pane: its command stream into the hub and its message stream out.
pub struct View {
    pub commands: tokio::sync::mpsc::UnboundedReceiver<Cmd>,
    pub ui: tokio::sync::mpsc::UnboundedSender<Msg>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Open {
    /// A new chat in the folder the window started in.
    New,
    /// A new chat in this folder's own checkout.
    Folder(std::path::PathBuf),
    /// A new topic on a new `summonr/<id>` branch in a git worktree of this folder's repository.
    Worktree(std::path::PathBuf),
    /// An earlier chat, in the folder it recorded.
    Resume(String),
    /// A new thread in the topic that session `topic` started, in that topic's folder.
    Thread {
        topic: String,
        folder: std::path::PathBuf,
    },
}

pub type Factory = Arc<dyn Fn(Open, Ui) -> Result<(Loop<Client>, Option<Workflow>)> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Working,
    NeedsYou,
    Review,
    Failed,
    Idle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Live {
    pub session: String,
    pub title: String,
    pub workspace: PathBuf,
    pub status: Status,
    pub active: bool,
    /// The lead that started this session, when it is a child.
    pub parent: Option<String>,
}

impl Status {
    pub const fn label(self) -> &'static str {
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
    if let Msg::Fact(Fact::Busy(activity)) = msg {
        return if activity.is_some() {
            Status::Working
        } else if matches!(prev, Status::Working | Status::NeedsYou) {
            Status::Review
        } else {
            prev
        };
    }
    if let Msg::Fact(Fact::Event(event)) = msg
        && let Some(asking) = asking(event)
    {
        return if asking {
            Status::NeedsYou
        } else if prev == Status::NeedsYou {
            Status::Working
        } else {
            prev
        };
    }
    if matches!(msg, Msg::Fact(Fact::Failed(_))) && prev == Status::Working {
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

fn asking(event: &Event) -> Option<bool> {
    match event {
        Event::Approval { detail, .. } => Some(detail.is_some()),
        Event::Elicit { form, .. } => Some(form.is_some()),
        _ => None,
    }
}
