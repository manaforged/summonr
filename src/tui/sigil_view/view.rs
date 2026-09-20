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
        let dialog = match screen.overlay() {
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
        };
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

pub(super) fn watch(app: &App) {
    drop((app.help.query.get(),));
    drop((
        app.search.open.get(),
        app.search.query.get(),
        app.search.caret.get(),
        app.search.selected.get(),
    ));
    drop((
        app.panes.get(),
        app.status.get(),
        app.select.get(),
        app.approval.get(),
        app.secret.get(),
        app.draft.get(),
        app.caret.get(),
        app.viewport.get(),
        app.scroll_y.get(),
        app.settings_sel.get(),
        app.hotkeys_open.get(),
        app.hotkey_sel.get(),
        app.hotkey_recording.get(),
        app.hotkey_note.get(),
        app.auth_open.get(),
        app.auth_choices.get(),
        app.auth_sel.get(),
        app.auth_entering.get(),
        app.auth_key.get(),
        app.auth_caret.get(),
        app.editing_model.get(),
        app.model_draft.get(),
        app.model_caret.get(),
        app.model_choices.get(),
        app.model_recent.get(),
        app.model_sel.get(),
        app.model_loading.get(),
        app.model_error.get(),
        app.workflow_open.get(),
        app.workflow_name.get(),
        app.workflow_names.get(),
        app.workflow_seats.get(),
        app.workflow_sel.get(),
        app.workflow_naming.get(),
        app.workflow_draft.get(),
        app.workflow_caret.get(),
        app.drag_from.get(),
        app.drag_over.get(),
        app.ticks.get(),
        app.view_edit.get(),
        app.view_text.get(),
        app.view_caret.get(),
        app.context_open.get(),
        app.cache_open.get(),
        app.meter_hover.get(),
    ));
}
