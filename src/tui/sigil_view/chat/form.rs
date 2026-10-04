use sigil::prelude::{NodeBuilder, Tone};

use super::App;
use crate::tui::sigil_view::app::Reply;
use crate::tui::sigil_view::ui::{Action, ActionKind, Hints, Key, Prompt};

pub(in crate::tui::sigil_view) fn form_band(
    app: App,
    width: usize,
    lines: Vec<String>,
    review: bool,
) -> NodeBuilder {
    let keys = app.model.peek().prefs.hotkeys.clone();
    let mut lines = lines.into_iter();
    let head = lines.next().unwrap_or_default();
    let prompt = lines.fold(Prompt::new(Tone::Info, head, hints(review)), Prompt::line);
    actions(app, review, prompt).build(width, &keys)
}

fn hints(review: bool) -> Hints {
    if review {
        Hints::new()
            .key(Key::Fixed("y"), "send")
            .key(Key::Fixed("n"), "decline")
            .key(Key::Fixed("e"), "edit")
            .key(Key::Esc, "cancel")
    } else {
        Hints::new().key(Key::Enter, "next").key(Key::Esc, "cancel")
    }
}

fn actions(app: App, review: bool, mut prompt: Prompt) -> Prompt {
    let reply = |kind: Reply| {
        let app = app.clone();
        move || app.form_reply(kind)
    };
    if review {
        prompt = prompt
            .action(Action::new("Send", ActionKind::Primary, reply(Reply::Send)))
            .action(Action::new(
                "Decline",
                ActionKind::Plain,
                reply(Reply::Decline),
            ));
    }
    prompt.action(Action::new(
        "Cancel",
        ActionKind::Destructive,
        reply(Reply::Cancel),
    ))
}
