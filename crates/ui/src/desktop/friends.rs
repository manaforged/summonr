use sigil::geometry::{Point, Rect};
use sigil::prelude::{LayoutStyle, NodeBuilder, Role, Widget};
use sigil::scene::{DrawRect, SceneBuilder};
use summonr_agent::endpoint::{Friend, friends};
use summonr_agent::recent::ModelId;

use super::App;
use super::style::mark;
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
    drop(sigil::tasks::spawn(async move {
        let mut seen = Vec::new();
        for friend in friends() {
            seen.push(check(friend).await);
        }
        drop(out.send(seen));
        Ok::<(), std::convert::Infallible>(())
    }));
}

/// Moves this chat to `model` on `friend`, after asking it whether it is signed in now.
pub(super) fn launch(app: &App, friend: Friend, model: String) {
    let owner = app.window.owner;
    let app = app.clone();
    let ask = move || {
        let (out, inbox) = tokio::sync::mpsc::unbounded_channel();
        sigil::tasks::on_message(inbox, move |seen| start(&app, seen, model.clone()), || {});
        drop(sigil::tasks::spawn(async move {
            drop(out.send(check(friend).await));
            Ok::<(), std::convert::Infallible>(())
        }));
    };
    match owner {
        Some(owner) => owner.with(ask),
        None => ask(),
    }
}

/// Acts on what `seen` says: the model when signed in, otherwise the step that gets there.
pub(super) fn start(app: &App, seen: Seen, model: String) {
    match seen.state {
        State::SignedIn => app.send_model(None, ModelId::new(seen.friend.route.to_owned(), model)),
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

/// The friend's icon in the status slot of a row.
pub(super) fn glyph(friend: Friend) -> NodeBuilder {
    let icon = mark(Status::Idle)
        .role(Role::Image)
        .label(friend.name)
        .child(
            sigil::widgets::canvas(move |scene, area| draw(friend, scene, area))
                .layout(LayoutStyle::block().fill()),
        );
    sigil::widgets::tooltip(friend.name).child(icon).into()
}

const fn quadrants(glyph: char) -> [bool; 4] {
    match glyph {
        '▘' => [true, false, false, false],
        '▝' => [false, true, false, false],
        '▖' => [false, false, true, false],
        '▗' => [false, false, false, true],
        '▀' => [true, true, false, false],
        '▄' => [false, false, true, true],
        '▌' => [true, false, true, false],
        '▐' => [false, true, false, true],
        '▛' => [true, true, true, false],
        '▜' => [true, true, false, true],
        '▙' => [true, false, true, true],
        '▟' => [false, true, true, true],
        '█' => [true; 4],
        _ => [false; 4],
    }
}

fn draw(friend: Friend, scene: &mut SceneBuilder, area: Rect) {
    let columns = friend.mark.iter().map(|row| row.chars().count()).max();
    let columns = columns.unwrap_or(1).max(1) as f32;
    let wide = area.width / (columns * 2.0);
    let tall = wide * 2.0;
    let height = friend.mark.len() as f32 * tall * 2.0;
    let top = area.origin.y + (area.height - height) / 2.0;
    let [red, green, blue] = friend.color;
    let color = crate::canvas::rgb(red, green, blue);
    for (y, row) in friend.mark.iter().enumerate() {
        for (x, glyph) in row.chars().enumerate() {
            for (at, on) in quadrants(glyph).into_iter().enumerate() {
                if on {
                    let left = (x * 2 + at % 2) as f32 * wide;
                    let down = (y * 2 + at / 2) as f32 * tall;
                    let origin = Point::new(area.origin.x + left, top + down);
                    scene.draw_rect(DrawRect::new(Rect::new(origin, wide, tall), color));
                }
            }
        }
    }
}
