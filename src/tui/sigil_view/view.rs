use crate::tui::Cmd;

pub(super) use super::canvas;
pub(super) use super::chat;
pub(super) use super::pane::Pane;
pub(super) use super::panes;
pub(super) use super::theme::Palette;

use super::app::{App, Overlay};
use super::scene;
use sigil::layout::Position;
use sigil::prelude::Key;
use sigil::prelude::KeyEvent;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::LogicalKey;
use sigil::prelude::ModifiersState;
use sigil::prelude::NodeBuilder;
use sigil::prelude::NodeEventsExt;
use sigil::prelude::NodeStyleExt;
use sigil::prelude::Paint;
use sigil::prelude::Rgba8;
use sigil::prelude::Role;
use sigil::prelude::Shortcut;
use sigil::prelude::Theme;
use sigil::prelude::WidgetLayoutExt;
use sigil::prelude::container;
use sigil::prelude::text_edit;
use sigil::style::ThemeValue;

mod input;
mod keys;
mod splash;
mod surface;

#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) use keys::shell;

pub(super) fn root(app: App) -> NodeBuilder {
    let screen = app.clone();
    keys::shell(&app).children_reactive(move || {
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
                .theme(surface::theme(&screen.theme_name.get())),
        ]
    })
}
