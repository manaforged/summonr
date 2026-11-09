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

    pub fn client(&self, endpoint: &crate::endpoint::Endpoint) -> crate::Result<crate::Client> {
        client(&self.0, endpoint)
    }
}

pub(crate) fn client(
    ui: &UnboundedSender<Msg>,
    endpoint: &crate::endpoint::Endpoint,
) -> crate::Result<crate::Client> {
    let ui = ui.clone();
    Ok(crate::Client::connect(endpoint)?.on_stream(move |delta| {
        let message = match delta {
            crate::Delta::Text(text) => Msg::Event(Event::Delta { text: text.clone() }),
            crate::Delta::Thinking(text) => Msg::Event(Event::Reasoning { text: text.clone() }),
            crate::Delta::Call(name) => Msg::Calling(name.clone()),
            crate::Delta::Progress => return,
        };
        drop(ui.send(message));
    }))
}
