use summonr_agent::hub::Cmd;

pub(super) use super::canvas;
pub(super) use super::chat;
pub(super) use super::pane::Pane;
pub(super) use super::panes;

use super::app::{App, Overlay};
use super::scene;
use sigil::prelude::Key;
use sigil::prelude::KeyEvent;
use sigil::prelude::LayoutStyle;
use sigil::prelude::LogicalKey;
use sigil::prelude::ModifiersState;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Paint;
use sigil::prelude::Position;
use sigil::prelude::Role;
use sigil::prelude::Shortcut;
use sigil::prelude::Widget;
use sigil::prelude::container;
use sigil::prelude::text_edit;

mod input;
pub(crate) mod keys;
mod splash;
pub(crate) mod surface;

pub(super) use keys::KeyCommand;

pub(super) fn root(app: App) -> NodeBuilder {
    let screen = app.clone();
    let outside = app.clone();
    keys::shell(&app)
        .pointer_down(move |_| outside.close_meters())
        .children_reactive(move || {
            let appearance = screen.appearance.get();
            let chat = if screen.splash.get() {
                splash::layer(screen.clone())
            } else {
                surface::layer(screen.clone())
            };
            let overlays = screen.clone();
            let dialog = container()
                .layout(LayoutStyle::block())
                .children_reactive(move || {
                    let _viewport = overlays.viewport.get();
                    let screen = overlays.clone();
                    vec![
                        screen
                            .overlay()
                            .and_then(|overlay| overlay.layer(screen.clone()))
                            .unwrap_or_else(container),
                    ]
                });
            vec![
                container()
                    .layout(LayoutStyle::block().fill_width().fill_height())
                    .bg(sigil::prelude::Token::Surface)
                    .child(chat)
                    .child(dialog)
                    .theme(super::theme::style(if screen.splash.get() {
                        super::theme::splash()
                    } else {
                        appearance
                    })),
            ]
        })
}
