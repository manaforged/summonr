use std::path::Path;

use tokio::sync::mpsc::UnboundedSender;

use crate::durable::log::{self, Line};
use crate::session::user_text;
use crate::tools::Event;

use super::{Msg, Worker};

impl Worker {
    pub(super) fn replay(&self) {
        self.restore();
    }

    pub(super) fn reshow(&self) {
        self.restore();
        drop(self.ui.send(Msg::Session(self.loop_.session_id())));
        drop(self.ui.send(Msg::Replayed));
    }

    fn restore(&self) {
        journal(self.loop_.session_path(), &self.ui);
        drop(self.ui.send(Msg::Event(Event::Unreported {
            tokens: self.loop_.unreported_tokens(),
        })));
    }
}

pub(super) fn reshow_running(path: &Path, session: &str, ui: &UnboundedSender<Msg>) {
    journal(path, ui);
    drop(ui.send(Msg::Session(session.to_string())));
    drop(ui.send(Msg::Replayed));
}

fn journal(path: &Path, ui: &UnboundedSender<Msg>) {
    let lines = match log::read(path) {
        Ok(lines) => lines,
        Err(error) => {
            drop(ui.send(Msg::System(format!("cannot load conversation: {error}"))));
            return;
        }
    };
    for line in lines {
        restore_line(line, ui);
    }
}

fn restore_line(line: Line, ui: &UnboundedSender<Msg>) {
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
