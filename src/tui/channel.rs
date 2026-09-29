use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::Msg;
use crate::tools::Event;

#[derive(Clone)]
pub struct Ui(pub(crate) UnboundedSender<Msg>);

pub struct Inbox(pub(crate) UnboundedReceiver<Msg>);

#[must_use]
pub fn channel() -> (Ui, Inbox) {
    let (tx, rx) = unbounded_channel();
    (Ui(tx), Inbox(rx))
}

impl Ui {
    pub fn open_login(&self, note: String) {
        drop(self.0.send(Msg::OpenLogin(note)));
    }

    pub fn secret(&self, request: crate::askpass::Request) {
        drop(self.0.send(Msg::Secret(request)));
    }

    pub fn event(&self, event: Event) {
        drop(self.0.send(Msg::Event(event)));
    }
}
