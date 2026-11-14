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

/// Whether another window asked for this session, telling the view when it did.
pub(super) fn released(path: &Path, ui: &UnboundedSender<Msg>) -> bool {
    let asked = log::release_asked(path);
    if asked {
        drop(ui.send(Msg::Fact(Fact::TakenOver)));
    }
    asked
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
    let mut out = Vec::new();
    if replace {
        out.push(Msg::Intent(Intent::Reset));
    }
    let mut last_work = None;
    for record in log::records(path, 0) {
        let line = match record {
            Ok(line) => line,
            Err(error) => {
                out.truncate(usize::from(replace));
                out.push(Msg::Fact(Fact::System(format!(
                    "cannot load conversation: {error}"
                ))));
                return out;
            }
        };
        restore_line(line, &mut last_work, &mut out);
    }
    if let Some(progress) = Work::progress_from(path, last_work) {
        out.push(Msg::Fact(Fact::Event(Event::Work {
            progress: crate::WorkProgress {
                reason: None,
                ..progress
            },
        })));
    }
    out
}

fn restore_line(line: Line, last_work: &mut Option<Work>, out: &mut Vec<Msg>) {
    match line {
        Line::User {
            text,
            source,
            images,
            start,
            ..
        } => {
            let text = user_text(&text, source.as_deref()).to_string();
            let message = if start || source.is_some() {
                Msg::Intent(Intent::User(text))
            } else {
                Msg::Fact(Fact::Injected(text))
            };
            out.push(message);
            for image in images {
                out.push(Msg::Intent(Intent::Image(image)));
            }
        }
        Line::Receipt {
            text,
            receipt: None,
        } => out.push(Msg::Fact(Fact::Receipt(text))),
        Line::Clear => out.push(Msg::Fact(Fact::Cleared)),
        Line::Work { work } => *last_work = Some(*work),
        line => {
            let interrupted = matches!(line, Line::Interrupted { .. });
            for event in line.into_events(crate::durable::projection::AssistantEvents::Include) {
                out.push(Msg::Fact(Fact::Event(event)));
            }
            if interrupted {
                out.push(Msg::Fact(Fact::System(
                    "Partial answer · interrupted".to_string(),
                )));
            }
        }
    }
}
