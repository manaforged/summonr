use sigil::prelude::*;

use super::super::Cmd;
use super::app::App;
use super::pane::Pane;
use super::view::shell;

mod icons;
mod options;
mod rail;
mod side;
mod thread;

pub(super) fn root(app: App) -> NodeBuilder {
    drop(app.cmd.send(Cmd::Diff(None)));
    let screen = app.clone();
    container()
        .layout(LayoutStyle::block().fill())
        .theme(Theme::dark())
        .children_reactive(move || {
            if screen.auth_open.get() {
                return vec![options::auth(screen.clone())];
            }
            if screen.search.open.get() {
                return vec![shell(&screen).child(super::search::desktop(screen.clone()))];
            }
            if screen.sessions.open.get() {
                return vec![shell(&screen).child(super::sessions::desktop(screen.clone()))];
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
                    vec![side::slot(panel.clone())]
                });
            vec![
                shell(&screen).child(
                    container()
                        .role(Role::Application)
                        .label("Summoner")
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

fn open(app: &App, pane: Pane) -> bool {
    app.layout.peek().secondary == Some(pane)
}

fn spacer() -> NodeBuilder {
    container().layout(LayoutStyle::block().grow(1.0))
}

fn hairline() -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fixed_size(1.0, 18.0))
        .class(classes!("bg-gray-6"))
}

pub(super) fn hit(name: String, on: bool, act: impl Fn() + 'static) -> NodeBuilder {
    container()
        .role(Role::Button)
        .label(name)
        .focusable(true)
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(28.0)
                .padding_x(8.0)
                .gap(8.0)
                .align_items(AlignItems::Center),
        )
        .class(classes!("rounded-md"))
        .class_when(signal(on), "bg-gray-5")
        .pointer_down(move |event| {
            act();
            event.stop_propagation();
        })
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
