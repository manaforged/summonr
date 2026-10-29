use std::path::Path;

use tokio::sync::mpsc::UnboundedSender;

use crate::durable::log::{self, Line};
use crate::durable::work::Work;
use crate::session::user_text;
use crate::tools::Event;

use super::{Msg, Worker};

impl Worker {
    pub(super) fn replay(&self) {
        let tokens = Some(self.loop_.unreported_tokens());
        replay_transcript(self.loop_.session_path(), None, tokens, &self.ui);
        if let Some((objective, _)) = self.loop_.active_goal().ok().flatten() {
            drop(self.ui.send(Msg::System(format!(
                "unfinished goal: {objective} · /goal with the same text continues it · /goal clear ends it"
            ))));
        }
    }
}

pub(super) fn replay_transcript(
    path: &Path,
    session: Option<String>,
    tokens: Option<u64>,
    ui: &UnboundedSender<Msg>,
) {
    journal(path, ui);
    if let Some(tokens) = tokens {
        drop(ui.send(Msg::Event(Event::Unreported { tokens })));
    }
    if let Some(session) = session {
        drop(ui.send(Msg::Session(session)));
        drop(ui.send(Msg::Replayed));
    }
}

fn journal(path: &Path, ui: &UnboundedSender<Msg>) {
    let lines = match log::records(path, 0).collect::<std::io::Result<Vec<Line>>>() {
        Ok(lines) => lines,
        Err(error) => {
            drop(ui.send(Msg::System(format!("cannot load conversation: {error}"))));
            return;
        }
    };
    for line in lines {
        restore_line(line, ui);
    }
    if let Some(work) = Work::at(path) {
        drop(ui.send(Msg::Event(Event::Work {
            progress: crate::WorkProgress {
                reason: None,
                ..work.progress()
            },
        })));
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
            let text = user_text(text, source.as_deref()).to_string();
            let message = if *start || source.is_some() {
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
