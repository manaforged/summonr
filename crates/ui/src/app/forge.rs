use sigil::prelude::{Signal, signal};
use summonr_agent::forge::{Ask, Error, Pull};

use super::{App, Cmd, Notice, NoticeKind};

/// The last forge answer for this pane's branch, and whether an Open PR or Merge is in flight.
#[derive(Clone, Copy)]
pub(crate) struct Forge {
    pub(crate) state: Signal<Option<Result<Option<Pull>, Error>>>,
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

    pub(super) fn forged(&self, reply: Result<Option<Pull>, Error>) {
        let asked = self.forge.asking.peek();
        self.forge.asking.set(false);
        match reply {
            Err(Error::Failed(text)) => self.forge_notice(text),
            Err(Error::NoRemote) if asked => {
                self.forge_notice(Error::NoRemote.to_string());
                self.forge.state.set(Some(Err(Error::NoRemote)));
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
