use std::path::Path;

use tokio::sync::mpsc::UnboundedSender;

use crate::durable::log::{self, Line};
use crate::durable::work::Work;
use crate::session::user_text;
use crate::tools::Event;

use super::{Msg, Worker};
use crate::hub::{Fact, Intent};

impl Worker {
    pub(super) fn replay(&self, replace: bool) {
        let tokens = Some(self.loop_.unreported_tokens());
        replay_transcript(self.loop_.session_path(), None, tokens, &self.ui, replace);
        if let Some((objective, _)) = self.loop_.active_goal().ok().flatten() {
            drop(self.ui.send(Msg::Fact(Fact::System(format!(
                "unfinished goal: {objective} · /goal with the same text continues it · /goal clear ends it"
            )))));
        }
    }
}

pub(super) fn replay_transcript(
    path: &Path,
    session: Option<String>,
    tokens: Option<u64>,
    ui: &UnboundedSender<Msg>,
    replace: bool,
) {
    drop(ui.send(Msg::Fact(Fact::Transcript(journal(path, replace)))));
    if let Some(tokens) = tokens {
        drop(ui.send(Msg::Fact(Fact::Event(Event::Unreported { tokens }))));
    }
    if let Some(session) = session {
        drop(ui.send(Msg::Fact(Fact::Session(session))));
        drop(ui.send(Msg::Fact(Fact::Replayed)));
    }
}

fn journal(path: &Path, replace: bool) -> Vec<Msg> {
    let lines = log::records(path, 0).collect::<std::io::Result<Vec<Line>>>();
    let mut out = Vec::new();
    if replace {
        out.push(Msg::Intent(Intent::Reset));
    }
    match lines {
        Ok(lines) => {
            for line in lines {
                restore_line(line, &mut out);
            }
        }
        Err(error) => {
            out.push(Msg::Fact(Fact::System(format!(
                "cannot load conversation: {error}"
            ))));
            return out;
        }
    }
    if let Some(work) = Work::at(path) {
        out.push(Msg::Fact(Fact::Event(Event::Work {
            progress: crate::WorkProgress {
                reason: None,
                ..work.progress()
            },
        })));
    }
    out
}

fn restore_line(line: Line, out: &mut Vec<Msg>) {
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
                Msg::Intent(Intent::User(text))
            } else {
                Msg::Fact(Fact::Injected(text))
            };
            out.push(message);
            for image in images {
                out.push(Msg::Intent(Intent::Image(image.clone())));
            }
            return;
        }
        Line::Receipt {
            text,
            receipt: None,
        } => {
            out.push(Msg::Fact(Fact::Receipt(text.clone())));
        }
        Line::Clear => out.push(Msg::Fact(Fact::Cleared)),
        _ => {}
    }
    for event in line.events() {
        out.push(Msg::Fact(Fact::Event(event)));
    }
    if matches!(line, Line::Interrupted { .. }) {
        out.push(Msg::Fact(Fact::System(
            "Partial answer · interrupted".to_string(),
        )));
    }
}
