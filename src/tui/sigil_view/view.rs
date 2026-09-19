use super::super::{Cmd, Msg};
use super::app::{App, Overlay};
use super::scene;
use super::settings;
use sigil::prelude::{
    Key, KeyEvent, LayoutBuilderExt, LayoutStyle, LogicalKey, ModifiersState, NodeBuilder,
    NodeEventsExt, NodeStyleExt, Position, Rgba8, Role, ScrollDelta, Shortcut, Style, Theme,
    ThemeValue, WidgetLayoutExt, container, text_edit,
};

mod chat;
mod input;
mod keys;
mod splash;

#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) use keys::shell;

const SCRIM: Rgba8 = Rgba8 {
    r: 10,
    g: 8,
    b: 20,
    a: 255,
};

pub(super) fn root(app: App) -> NodeBuilder {
    let screen = app.clone();
    keys::shell(&app).children_reactive(move || {
        if let Some(entry) = screen.search.target.peek() {
            match screen.history.borrow_mut().position(&screen, entry) {
                Ok(Some(position)) => screen.scroll_y.set(position),
                Ok(None) => {}
                Err(error) => screen.update(Msg::System(error)),
            }
            screen.search.target.set(None);
        }
        let view = match screen.overlay() {
            Some(Overlay::Help) => super::help::layer(screen.clone()),
            Some(Overlay::Search) => super::search::layer(screen.clone()),
            Some(Overlay::Sessions) => super::sessions::layer(screen.clone()),
            Some(Overlay::Usage) => super::usage::layer(screen.clone()),
            Some(Overlay::Auth) => settings::auth_layer(screen.clone()),
            Some(Overlay::Model) => settings::model_picker_layer(screen.clone()),
            Some(Overlay::Workflow) => settings::workflow_layer(screen.clone()),
            Some(Overlay::Hotkeys) => settings::hotkeys_layer(screen.clone()),
            Some(Overlay::Settings) => settings::settings_layer(screen.clone()),
            None if screen.splash.get() => splash::layer(screen.clone()),
            Some(Overlay::Context | Overlay::Cache) | None => chat::layer(screen.clone()),
        };
        vec![
            container()
                .layout(LayoutStyle::block().fill_width().fill_height())
                .child(view)
                .theme(chat::theme(&screen.theme_name.get())),
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
