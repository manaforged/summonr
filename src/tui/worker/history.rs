use crate::durable::log::{self, Line};
use crate::session::user_text;
use crate::tools::Event;

use super::{Msg, Worker};

impl Worker {
    pub(super) fn replay(&self) {
        let lines = match log::read(self.loop_.session_path()) {
            Ok(lines) => lines,
            Err(error) => {
                self.system(&format!("cannot load conversation: {error}"));
                return;
            }
        };
        let charged = lines
            .iter()
            .any(|line| matches!(line, Line::Assistant { .. } | Line::Charge { .. }));
        for line in lines {
            self.restore(line);
        }
        drop(self.ui.send(Msg::Event(Event::Unreported {
            tokens: self.loop_.unreported_tokens(),
        })));
        if charged {
            self.system(
                "Restored token usage. Historical reasoning tokens and cost are unavailable.",
            );
        }
    }

    fn restore(&self, line: Line) {
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
                drop(self.ui.send(message));
                for image in images {
                    drop(self.ui.send(Msg::Image(image.clone())));
                }
                return;
            }
            Line::Receipt {
                text,
                receipt: None,
            } => {
                drop(self.ui.send(Msg::Receipt(text.clone())));
            }
            Line::Clear => drop(self.ui.send(Msg::Cleared)),
            _ => {}
        }
        for event in line.events() {
            drop(self.ui.send(Msg::Event(event)));
        }
        if matches!(line, Line::Interrupted { .. }) {
            self.system("Partial answer · interrupted");
        }
    }
}
