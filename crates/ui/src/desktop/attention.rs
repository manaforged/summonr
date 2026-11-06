use summonr_agent::hub::{Fact, Msg};

use super::App;
use crate::model::Entry;

const ASKS: &str = "Summonr needs your OK";
const QUESTION: &str = "Summonr has a question";
const FINISHED: &str = "Summonr finished";

impl App {
    /// The notification a message deserves when the user is away: a title and a body.
    pub(crate) fn attention(&self, msg: &Msg) -> Option<(String, String)> {
        let place = summonr_agent::worktree::display_name(&self.workspace()).unwrap_or_default();
        if let Msg::Fact(Fact::Event(summonr_agent::Event::Approval {
            detail: Some(detail),
            ..
        })) = msg
        {
            return Some((
                ASKS.to_owned(),
                detail.lines().next().unwrap_or_default().to_owned(),
            ));
        }
        if matches!(
            msg,
            Msg::Fact(Fact::Event(summonr_agent::Event::Elicit { .. }))
        ) {
            return Some((QUESTION.to_owned(), format!("Answer it in {place}")));
        }
        (matches!(msg, Msg::Fact(Fact::Busy(None))) && self.model.peek().busy_since.is_some())
            .then(|| (FINISHED.to_owned(), format!("Your turn in {place}")))
    }

    pub(super) fn call_attention(&self, msg: &Msg) {
        if self.window.focused.is_none_or(|focused| focused.peek()) {
            return;
        }
        let Some((title, body)) = self.attention(msg) else {
            return;
        };
        let sent = sigil::app::notify::Notification::new(title, body)
            .app_name("Summonr")
            .send();
        if let Err(error) = sent {
            self.push_entry(Entry::System(format!(
                "Could not show a notification: {error}"
            )));
        }
    }
}
