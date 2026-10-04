use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Theme, app_shell, container,
};

use super::ui::{Hints, Key};
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
            let keys = screen.model.peek().prefs.hotkeys.clone();
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
                                side.hotkeys.open.get(),
                                side.hotkeys.sel.get(),
                                side.hotkeys.recording.get(),
                                side.hotkeys.note.get(),
                                side.picker.open.get(),
                                side.picker.draft.get(),
                                side.picker.choices.get(),
                                side.picker.sel.get(),
                                side.picker.loading.get(),
                                side.picker.error.get(),
                                side.workflow.open.get(),
                                side.workflow.name.get(),
                                side.workflow.names.get(),
                                side.workflow.seats.get(),
                                side.workflow.sel.get(),
                                side.workflow.naming.get(),
                                side.workflow.draft.get(),
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
                Hints::new().key(Key::Esc, "closes overlays").bar(&keys),
                overlay(&screen),
            )]
        })
}

fn overlay(app: &App) -> Option<NodeBuilder> {
    app.overlay().and_then(|overlay| overlay.layer(app.clone()))
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
