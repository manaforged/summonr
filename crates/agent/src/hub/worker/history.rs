use std::path::Path;

use tokio::sync::mpsc::UnboundedSender;

use crate::durable::log::{self, Line};
use crate::durable::work::Work;
use crate::session::user_text;
use crate::tools::Event;

use super::{Msg, Worker};
use crate::hub::Fact;

impl Worker {
    pub(super) fn replay(&self, replace: bool) {
        let tokens = Some(self.loop_.unreported_tokens());
        replay_transcript(self.loop_.session_path(), None, tokens, &self.ui, replace);
        if let Some((objective, _)) = self.loop_.active_goal().ok().flatten() {
            drop(self.ui.send(Msg::Fact(Fact::Event(Event::Note {
                text: format!(
                    "unfinished goal: {objective} · /goal with the same text continues it · /goal clear ends it"
                ),
            }))));
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
    let events = journal(path);
    drop(ui.send(Msg::Fact(Fact::Transcript { replace, events })));
    if let Some(tokens) = tokens {
        drop(ui.send(Msg::Fact(Fact::Event(Event::Unreported { tokens }))));
    }
    if let Some(session) = session {
        drop(ui.send(Msg::Fact(Fact::Session(session))));
        drop(ui.send(Msg::Fact(Fact::Replayed)));
    }
}

fn journal(path: &Path) -> Vec<Event> {
    let mut out = Vec::new();
    let mut last_work = None;
    for record in log::records(path, 0) {
        let line = match record {
            Ok(line) => line,
            Err(error) => {
                return vec![Event::Note {
                    text: format!("cannot load conversation: {error}"),
                }];
            }
        };
        restore_line(line, &mut last_work, &mut out);
    }
    if let Some(progress) = Work::progress_from(path, last_work) {
        out.push(Event::Work {
            progress: crate::WorkProgress {
                reason: None,
                ..progress
            },
        });
    }
    out
}

fn restore_line(line: Line, last_work: &mut Option<Work>, out: &mut Vec<Event>) {
    match line {
        Line::User {
            text,
            source,
            images,
            start,
            ..
        } => {
            let text = user_text(&text, source.as_deref()).to_string();
            out.push(if start || source.is_some() {
                Event::User { text, images }
            } else {
                Event::Injected { text }
            });
        }
        Line::Receipt {
            text,
            receipt: None,
        } => {
            if !text.is_empty() {
                out.push(Event::Receipt {
                    receipt: crate::verify::Receipt {
                        text,
                        ..crate::verify::Receipt::default()
                    },
                });
            }
        }
        Line::Clear => out.push(Event::Cleared),
        Line::Work { work } => *last_work = Some(*work),
        line => {
            let interrupted = matches!(line, Line::Interrupted { .. });
            out.extend(line.into_events(crate::durable::projection::AssistantEvents::Include));
            if interrupted {
                out.push(Event::Note {
                    text: "Partial answer · interrupted".to_string(),
                });
            }
        }
    }
}
