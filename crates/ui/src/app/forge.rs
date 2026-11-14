use sigil::prelude::{Signal, signal};
#[cfg(feature = "gui")]
use summonr_agent::forge::Ask;
use summonr_agent::forge::{Failure, Pull};

#[cfg(feature = "gui")]
use super::Cmd;
use super::{App, Notice, NoticeKind};

/// The last forge answer for this pane's branch, and whether an Open PR or Merge is in flight.
#[derive(Clone, Copy)]
pub(crate) struct Forge {
    pub(crate) state: Signal<Option<Result<Option<Pull>, Failure>>>,
    pub(crate) asking: Signal<bool>,
}

impl Forge {
    pub(super) fn new() -> Self {
        Self {
            state: signal(None),
            asking: signal(false),
        }
    }
}

impl App {
    #[cfg(feature = "gui")]
    /// Sends `ask` to the hub; Open PR and Merge hold the buttons until the answer.
    pub(crate) fn forge_ask(&self, ask: Ask) {
        if ask != Ask::Status {
            if self.forge.asking.peek() {
                return;
            }
            self.forge.asking.set(true);
        }
        drop(self.cmd.send(Cmd::Forge(ask)));
    }

    pub(super) fn forged(&self, reply: Result<Option<Pull>, Failure>) {
        let asked = self.forge.asking.peek();
        self.forge.asking.set(false);
        match reply {
            Err(Failure::Failed(text)) => self.forge_notice(text),
            Err(Failure::NoRemote) if asked => {
                self.forge_notice(Failure::NoRemote.to_string());
                self.forge.state.set(Some(Err(Failure::NoRemote)));
            }
            reply => self.forge.state.set(Some(reply)),
        }
    }

    fn forge_notice(&self, text: String) {
        self.notice.set(Some(Notice {
            text,
            kind: NoticeKind::Failed,
        }));
    }
}
