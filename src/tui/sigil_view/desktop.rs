use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Role, Theme, classes, container,
};

use super::super::Cmd;
use super::app::{App, Overlay};
use super::pane::Pane;
use super::view::shell;

mod icons;
mod rail;
mod thread;

pub(super) fn root(app: App) -> NodeBuilder {
    drop(app.cmd.send(Cmd::Diff(None)));
    let screen = app.clone();
    container()
        .layout(LayoutStyle::block().fill())
        .theme(Theme::dark())
        .children_reactive(move || {
            let overlay = match screen.overlay() {
                Some(Overlay::Help) => Some(super::help::layer(screen.clone())),
                Some(Overlay::Search) => Some(super::search::layer(screen.clone())),
                Some(Overlay::Sessions) => Some(super::sessions::layer(screen.clone())),
                Some(Overlay::Usage) => Some(super::usage::layer(screen.clone())),
                Some(Overlay::Auth) => Some(super::settings::auth_layer(screen.clone())),
                Some(Overlay::Model) => Some(super::settings::model_picker_layer(screen.clone())),
                Some(Overlay::Workflow) => Some(super::settings::workflow_layer(screen.clone())),
                Some(Overlay::Settings) => Some(super::settings::settings_layer(screen.clone())),
                Some(Overlay::Hotkeys) => Some(super::settings::hotkeys_layer(screen.clone())),
                Some(Overlay::Context | Overlay::Cache) | None => None,
            };
            if let Some(overlay) = overlay {
                return vec![shell(&screen).child(overlay)];
            }
            let navigation = screen.clone();
            let panel = screen.clone();
            let sidebar = container()
                .layout(LayoutStyle::block().fill_height().shrink(0.0))
                .children_reactive(move || {
                    drop((
                        panel.status.get(),
                        panel.layout.get(),
                        panel.panes.get(),
                        panel.pane_focus.get(),
                        panel.settings_open.get(),
                        panel.settings_sel.get(),
                        panel.hotkeys_open.get(),
                        panel.hotkey_sel.get(),
                        panel.hotkey_recording.get(),
                        panel.hotkey_note.get(),
                        panel.editing_model.get(),
                        panel.model_draft.get(),
                        panel.model_choices.get(),
                        panel.model_sel.get(),
                        panel.model_loading.get(),
                        panel.model_error.get(),
                        panel.workflow_open.get(),
                        panel.workflow_name.get(),
                        panel.workflow_names.get(),
                        panel.workflow_seats.get(),
                        panel.workflow_sel.get(),
                        panel.workflow_naming.get(),
                        panel.workflow_draft.get(),
                        panel.diff_view.get(),
                        panel.view_edit.get(),
                    ));
                    vec![sidebar(panel.clone())]
                });
            vec![
                shell(&screen).child(
                    container()
                        .role(Role::Application)
                        .label("Summonr")
                        .layout(LayoutStyle::flex_row().fill())
                        .class(classes!("bg-gray-1"))
                        .child(
                            container()
                                .layout(LayoutStyle::block().fill_height().shrink(0.0))
                                .children_reactive(move || {
                                    let _updates =
                                        (navigation.panes.get(), navigation.layout.get());
                                    vec![rail::rail(navigation.clone())]
                                }),
                        )
                        .child(thread::main_col(screen.clone()))
                        .child(sidebar),
                ),
            ]
        })
}

fn sidebar(app: App) -> NodeBuilder {
    let pane = match app.overlay() {
        Some(Overlay::Model) | Some(Overlay::Workflow) => None,
        Some(
            Overlay::Help
            | Overlay::Search
            | Overlay::Sessions
            | Overlay::Usage
            | Overlay::Auth
            | Overlay::Hotkeys
            | Overlay::Settings
            | Overlay::Context
            | Overlay::Cache,
        )
        | None => app.layout.peek().secondary,
    };
    match pane {
        Some(pane) => super::panes::body::pane_body(app, pane, 360.0),
        None => container(),
    }
}

fn open(app: &App, pane: Pane) -> bool {
    app.layout.peek().secondary == Some(pane)
}
