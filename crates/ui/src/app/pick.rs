use summonr_agent::runtime::GrantAnswer;

use super::elicit::Reply;
use super::{App, Stage};

/// One answer the user can give to the pending approval or question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    Grant(GrantAnswer),
    Option(usize),
    Reply(Reply),
}

impl App {
    /// The answers to the pending approval or question, in key order 1, 2, 3.
    pub(crate) fn picks(&self) -> Vec<(String, Pick)> {
        if let Some(asked) = self.approval.get() {
            let grants: &[(&str, GrantAnswer)] = if asked.persist {
                &[
                    ("Allow for this agent", GrantAnswer::Allow),
                    ("Always allow", GrantAnswer::Always),
                    ("Don't allow", GrantAnswer::Deny),
                ]
            } else {
                &[
                    ("Allow once", GrantAnswer::Allow),
                    ("Don't allow", GrantAnswer::Deny),
                ]
            };
            return grants
                .iter()
                .map(|(label, answer)| ((*label).to_string(), Pick::Grant(*answer)))
                .collect();
        }
        let Some(state) = self.form.get() else {
            return Vec::new();
        };
        if state.stage == Stage::Review {
            return [
                ("Send", Reply::Send),
                ("Decline", Reply::Decline),
                ("Cancel", Reply::Cancel),
            ]
            .into_iter()
            .map(|(label, reply)| (label.to_string(), Pick::Reply(reply)))
            .collect();
        }
        state
            .choice_titles()
            .into_iter()
            .enumerate()
            .map(|(index, title)| (title, Pick::Option(index)))
            .collect()
    }

    /// Gives `pick` as the answer.
    pub(crate) fn pick(&self, pick: Pick) {
        match pick {
            Pick::Grant(answer) => self.approval_answer(answer),
            Pick::Reply(reply) => self.form_reply(reply),
            Pick::Option(index) => self.form_pick(index),
        }
    }

    /// Gives the answer at `number` (1-based) when the composer is empty.
    pub(crate) fn pick_number(&self, number: usize) -> bool {
        if !self.draft.peek().is_empty() || self.options_active() {
            return false;
        }
        let Some((_, pick)) = number
            .checked_sub(1)
            .and_then(|index| self.picks().into_iter().nth(index))
        else {
            return false;
        };
        self.pick(pick);
        true
    }
}
