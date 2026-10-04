use std::path::Path;

use tokio::sync::mpsc::UnboundedSender;

use crate::durable::log::{self, Line};
use crate::session::user_text;
use crate::tools::Event;

use super::{Msg, Worker};

const CHARGED: &str = "Restored token usage. Historical reasoning tokens and cost are unavailable.";

impl Worker {
    pub(super) fn replay(&self) {
        let charged = journal(self.loop_.session_path(), &self.ui);
        drop(self.ui.send(Msg::Event(Event::Unreported {
            tokens: self.loop_.unreported_tokens(),
        })));
        if charged {
            self.system(CHARGED);
        }
    }

    pub(super) fn reshow(&self) {
        self.replay();
        drop(self.ui.send(Msg::Session(self.loop_.session_id())));
        drop(self.ui.send(Msg::Replayed));
    }
}

pub(super) fn reshow_running(path: &Path, session: &str, ui: &UnboundedSender<Msg>) {
    if journal(path, ui) {
        drop(ui.send(Msg::System(CHARGED.to_string())));
    }
    drop(ui.send(Msg::Session(session.to_string())));
    drop(ui.send(Msg::Replayed));
}

fn journal(path: &Path, ui: &UnboundedSender<Msg>) -> bool {
    let lines = match log::read(path) {
        Ok(lines) => lines,
        Err(error) => {
            drop(ui.send(Msg::System(format!("cannot load conversation: {error}"))));
            return false;
        }
    };
    let charged = lines
        .iter()
        .any(|line| matches!(line, Line::Assistant { .. } | Line::Charge { .. }));
    for line in lines {
        restore(line, ui);
    }
    charged
}

fn restore(line: Line, ui: &UnboundedSender<Msg>) {
    match &line {
        Line::User {
            text,
            source,
            images,
            start,
            ..
        } => {
            let text = source
                .clone()
                .unwrap_or_else(|| user_text(text).to_string());
            let message = if *start {
                Msg::User(text)
            } else {
                Msg::System(text)
            };
            drop(ui.send(message));
            for image in images {
                drop(ui.send(Msg::Image(image.clone())));
            }
            return;
        }
        Line::Receipt {
            text,
            receipt: None,
        } => {
            drop(ui.send(Msg::Receipt(text.clone())));
        }
        Line::Clear => drop(ui.send(Msg::Cleared)),
        _ => {}
    }
    for event in line.events() {
        drop(ui.send(Msg::Event(event)));
    }
    if matches!(line, Line::Interrupted { .. }) {
        drop(ui.send(Msg::System("Partial answer · interrupted".to_string())));
    }
}
