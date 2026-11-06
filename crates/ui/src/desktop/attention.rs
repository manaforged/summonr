use summonr_agent::hub::{Fact, Msg, Status};
use summonr_agent::prefs::Notify;

use super::App;
use crate::app::{Notice, NoticeKind};

const ASKS: &str = "Needs you";
const QUESTION: &str = "Summonr has a question";
const FINISHED: &str = "Summonr finished";
const NEEDS_YOU: &str = "Needs you";
const DONE: &str = "Finished";

/// What an agent out of view says when it reaches `status`, if `notify` allows it.
pub(super) fn alert(status: Status, notify: Notify) -> Option<&'static str> {
    match status {
        Status::NeedsYou => notify.approvals().then_some(NEEDS_YOU),
        Status::Review => notify.turns().then_some(DONE),
        Status::Working | Status::Failed | Status::Idle => None,
    }
}

/// Sends a desktop notification when an agent out of view starts to need the user or finishes.
pub(super) fn announce(app: &App) {
    let watch = app.clone();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Status)>::new()));
    sigil::prelude::effect(move || {
        let model = watch.model.get();
        let mut before = seen.borrow_mut();
        for entry in &model.live {
            let was = before
                .iter()
                .find(|(id, _)| *id == entry.session)
                .map(|(_, status)| *status);
            let Some(word) = alert(entry.status, model.prefs.notify) else {
                continue;
            };
            if was.is_some_and(|was| was != entry.status) && !super::split::shows(&entry.session) {
                watch.notify_user(entry.title.clone(), word.to_owned());
            }
        }
        *before = model
            .live
            .iter()
            .map(|entry| (entry.session.clone(), entry.status))
            .collect();
    });
}

impl App {
    /// The notification a message deserves when the user is away: a title and a body.
    pub(crate) fn attention(&self, msg: &Msg) -> Option<(String, String)> {
        let notify = self.model.peek().prefs.notify;
        let place = summonr_agent::worktree::display_name(&self.workspace()).unwrap_or_default();
        if let Msg::Fact(Fact::Event(summonr_agent::Event::Approval {
            detail: Some(detail),
            ..
        })) = msg
        {
            return notify.approvals().then(|| {
                (
                    ASKS.to_owned(),
                    detail.lines().next().unwrap_or_default().to_owned(),
                )
            });
        }
        if matches!(
            msg,
            Msg::Fact(Fact::Event(summonr_agent::Event::Elicit { .. }))
        ) {
            return notify
                .approvals()
                .then(|| (QUESTION.to_owned(), format!("Answer it in {place}")));
        }
        (matches!(msg, Msg::Fact(Fact::Busy(None)))
            && self.model.peek().busy_since.is_some()
            && notify.turns())
        .then(|| (FINISHED.to_owned(), format!("Your turn in {place}")))
    }

    pub(super) fn call_attention(&self, msg: &Msg) {
        if self.window.focused.is_none_or(|focused| focused.peek()) {
            return;
        }
        if let Some((title, body)) = self.attention(msg) {
            self.notify_user(title, body);
        }
    }

    fn notify_user(&self, title: String, body: String) {
        let sent = sigil::app::notify::Notification::new(title, body)
            .app_name("Summonr")
            .send();
        if let Err(error) = sent {
            self.notice.set(Some(Notice {
                text: format!("Could not show a notification: {error}"),
                kind: NoticeKind::Failed,
            }));
        }
    }
}
