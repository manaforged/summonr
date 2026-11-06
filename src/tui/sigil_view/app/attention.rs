use super::*;

const ASKS: &str = "Summonr needs your OK";
const QUESTION: &str = "Summonr has a question";
const FINISHED: &str = "Summonr finished";

impl App {
    /// The notification a message deserves when the user is away: a title and a body.
    pub(in crate::tui::sigil_view) fn attention(&self, msg: &Msg) -> Option<(String, String)> {
        let place = crate::worktree::display_name(&self.workspace()).unwrap_or_default();
        match msg {
            Msg::Fact(Fact::Event(crate::Event::Approval { detail: Some(detail), .. })) => {
                Some((ASKS.to_owned(), detail.lines().next().unwrap_or_default().to_owned()))
            }
            Msg::Fact(Fact::Event(crate::Event::Elicit { .. })) => {
                Some((QUESTION.to_owned(), format!("Answer it in {place}")))
            }
            Msg::Fact(Fact::Busy(None)) if self.model.peek().busy_since.is_some() => {
                Some((FINISHED.to_owned(), format!("Your turn in {place}")))
            }
            _ => None,
        }
    }

    pub(super) fn call_attention(&self, msg: &Msg) {
        if self.window_focused.is_none_or(|focused| focused.peek()) {
            return;
        }
        let Some((title, body)) = self.attention(msg) else {
            return;
        };
        let sent = sigil::app::notify::Notification::new(title, body).app_name("Summonr").send();
        if let Err(error) = sent {
            self.push_entry(Entry::System(format!("Could not show a notification: {error}")));
        }
    }
}
