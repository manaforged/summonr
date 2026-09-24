use crate::tui::Cmd;

pub(super) use super::canvas;
pub(super) use super::chat;
pub(super) use super::pane::Pane;
pub(super) use super::panes;
pub(super) use super::theme::Palette;

use super::app::{App, Overlay};
use super::scene;
use super::settings;
use sigil::prelude::{
    Key, KeyEvent, LayoutBuilderExt, LayoutStyle, LogicalKey, ModifiersState, NodeBuilder,
    NodeEventsExt, NodeStyleExt, Position, Rgba8, Role, ScrollDelta, Shortcut, Style, Theme,
    ThemeValue, WidgetLayoutExt, container, text_edit,
};

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
                vec![match screen.overlay() {
                    Some(Overlay::Help) => super::help::layer(screen.clone()),
                    Some(Overlay::Search) => super::search::layer(screen.clone()),
                    Some(Overlay::Sessions) => super::sessions::layer(screen.clone()),
                    Some(Overlay::Usage) => super::usage::layer(screen.clone()),
                    Some(Overlay::Auth) => settings::auth_layer(screen.clone()),
                    Some(Overlay::Model) => settings::model_picker_layer(screen.clone()),
                    Some(Overlay::Workflow) => settings::workflow_layer(screen.clone()),
                    Some(Overlay::Hotkeys) => settings::hotkeys_layer(screen.clone()),
                    Some(Overlay::Settings) => settings::settings_layer(screen.clone()),
                    Some(Overlay::Context | Overlay::Cache) | None => container(),
                }]
            });
        vec![
            container()
                .layout(LayoutStyle::block().fill_width().fill_height())
                .class("w-full h-full bg-surface")
                .child(chat)
                .child(dialog)
                .theme(surface::theme(&screen.theme_name.get())),
        ]
    })
}
