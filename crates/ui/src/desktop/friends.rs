use std::rc::Rc;

use sigil::prelude::{LayoutStyle, ListRowSlots, NodeBuilder, Widget, container, list_row};
use summonr_agent::endpoint::{Friend, friends};
use summonr_agent::hub::Open;

use super::App;
use super::style::{Text, mark, text};
use summonr_agent::hub::Status;

/// What this machine says about a friend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Seen {
    /// The friend.
    pub(crate) friend: Friend,
    /// Where it stands here.
    pub(crate) state: State,
}

/// Whether a friend can run a chat here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Missing,
    SignedOut,
    SignedIn,
}

async fn check(friend: Friend) -> Seen {
    let state = if !friend.installed() {
        State::Missing
    } else if friend.signed_in().await {
        State::SignedIn
    } else {
        State::SignedOut
    };
    Seen { friend, state }
}

/// Asks every friend where it stands and publishes the answers.
pub(super) fn refresh(app: &App) {
    let list = app.window.friends;
    let (out, inbox) = tokio::sync::mpsc::unbounded_channel();
    sigil::tasks::on_message(inbox, move |seen| list.set(seen), || {});
    drop(tokio::spawn(async move {
        let mut seen = Vec::new();
        for friend in friends() {
            seen.push(check(friend).await);
        }
        drop(out.send(seen));
    }));
}

/// Opens a chat on `friend`, after asking it whether it is signed in now.
pub(super) fn launch(app: &App, friend: Friend) {
    let app = app.clone();
    let (out, inbox) = tokio::sync::mpsc::unbounded_channel();
    sigil::tasks::on_message(inbox, move |seen| start(&app, seen), || {});
    drop(tokio::spawn(async move {
        drop(out.send(check(friend).await));
    }));
}

/// Acts on what `seen` says: a chat when signed in, otherwise the step that gets there.
pub(super) fn start(app: &App, seen: Seen) {
    match seen.state {
        State::SignedIn => {
            super::split::show(app, Open::Agent(seen.friend.route.to_owned()));
        }
        State::SignedOut => sign_in(app, seen.friend),
        State::Missing => app.settings_open.set(true),
    }
}

/// Runs the friend's own sign-in in a terminal, so its flow owns the credentials.
pub(super) fn sign_in(app: &App, friend: Friend) {
    app.settings_open.set(false);
    app.open_terminal(friend.name, Some(friend.login));
}

/// Runs the vendor's install command in a terminal.
pub(super) fn install(app: &App, friend: Friend) {
    app.settings_open.set(false);
    app.open_terminal(friend.name, Some(friend.install));
}

/// The sidebar row of each installed friend.
pub(super) fn rows(app: &App) -> NodeBuilder {
    let app = app.clone();
    container()
        .layout(LayoutStyle::block().fill_width())
        .children_reactive(move || {
            app.window
                .friends
                .get()
                .iter()
                .filter(|seen| seen.state != State::Missing)
                .map(|seen| row(&app, seen.friend))
                .collect::<Vec<_>>()
        })
}

fn row(app: &App, friend: Friend) -> NodeBuilder {
    let app = app.clone();
    list_row(
        false,
        format!("New {}", friend.name),
        ListRowSlots {
            leading: Some(Box::new(glyph(friend))),
            activate: Some(Rc::new(move || launch(&app, friend))),
            ..ListRowSlots::default()
        },
    )
}

/// The friend's mark in the status slot of a row.
pub(super) fn glyph(friend: Friend) -> NodeBuilder {
    sigil::widgets::tooltip(friend.name)
        .child(mark(Status::Idle).child(text(Text::Meta, friend.mark)))
        .into()
}
