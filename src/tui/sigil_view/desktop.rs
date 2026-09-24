use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Theme, app_shell, container, hint_bar,
};

use super::app::Overlay;
use super::view::shell;
use crate::tui::Cmd;

pub(super) use super::app::App;
pub(super) use super::chat;
pub(super) use super::pane::Pane;

mod icons;
mod rail;
mod thread;

pub(super) fn root(app: App) -> NodeBuilder {
    drop(app.cmd.send(Cmd::Diff(None)));
    let screen = app.clone();
    shell(&screen)
        .theme(Theme::dark())
        .children_reactive(move || {
            let chrome = screen.clone();
            let navigation = screen.clone();
            let side = screen.clone();
            let top = screen.clone();
            let work = container()
                .layout(LayoutStyle::flex_row().fill())
                .child(
                    container()
                        .layout(LayoutStyle::block().fill_height().shrink(0.0))
                        .children_reactive(move || {
                            let _updates = navigation.layout.get();
                            vec![rail::rail(navigation.clone())]
                        }),
                )
                .child(thread::main_col(chrome.clone()))
                .child(
                    container()
                        .layout(LayoutStyle::block().fill_height().shrink(0.0))
                        .children_reactive(move || {
                            drop((
                                side.status.get(),
                                side.layout.get(),
                                side.pane_focus.get(),
                                side.settings_open.get(),
                                side.settings_sel.get(),
                                side.hotkeys_open.get(),
                                side.hotkey_sel.get(),
                                side.hotkey_recording.get(),
                                side.hotkey_note.get(),
                                side.editing_model.get(),
                                side.model_draft.get(),
                                side.model_choices.get(),
                                side.model_sel.get(),
                                side.model_loading.get(),
                                side.model_error.get(),
                                side.workflow_open.get(),
                                side.workflow_name.get(),
                                side.workflow_names.get(),
                                side.workflow_seats.get(),
                                side.workflow_sel.get(),
                                side.workflow_naming.get(),
                                side.workflow_draft.get(),
                                side.diff_view.get(),
                                side.view_edit.get(),
                            ));
                            vec![pane(side.clone())]
                        }),
                );
            vec![app_shell(
                container()
                    .layout(LayoutStyle::block().fill_width())
                    .children_reactive(move || {
                        let _updates = top.layout.get();
                        vec![thread::title_bar(top.clone())]
                    }),
                work,
                hint_bar("esc closes overlays", String::new()),
                overlay(&screen),
            )]
        })
}

fn overlay(app: &App) -> Option<NodeBuilder> {
    match app.overlay() {
        Some(Overlay::Help) => Some(super::help::layer(app.clone())),
        Some(Overlay::Search) => Some(super::search::layer(app.clone())),
        Some(Overlay::Sessions) => Some(super::sessions::layer(app.clone())),
        Some(Overlay::Usage) => Some(super::usage::layer(app.clone())),
        Some(Overlay::Auth) => Some(super::settings::auth_layer(app.clone())),
        Some(Overlay::Model) => Some(super::settings::model_picker_layer(app.clone())),
        Some(Overlay::Workflow) => Some(super::settings::workflow_layer(app.clone())),
        Some(Overlay::Settings) => Some(super::settings::settings_layer(app.clone())),
        Some(Overlay::Hotkeys) => Some(super::settings::hotkeys_layer(app.clone())),
        Some(Overlay::Context | Overlay::Cache) | None => None,
    }
}

fn pane(app: App) -> NodeBuilder {
    match app.layout.peek().secondary {
        Some(pane) => super::panes::body::pane_body(app, pane, 360.0),
        None => container().layout(LayoutStyle::block().fixed_size(0.0, 0.0).shrink(0.0)),
    }
}

fn open(app: &App, pane: Pane) -> bool {
    app.layout.peek().secondary == Some(pane)
}
