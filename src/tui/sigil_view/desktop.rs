//! The desktop window: Codex-shaped chrome over the same [`App`] the
//! terminal drives. A left rail switches panes and lists sessions, the main
//! column holds the transcript and composer, the right rail shows the open
//! pane with real widgets.

use sigil::prelude::*;

use super::super::Cmd;
use super::app::App;
use super::pane::Pane;

mod icons;
mod rail;
mod side;
mod thread;

pub(super) fn root(app: App) -> NodeBuilder {
    drop(app.cmd.send(Cmd::Diff(None)));
    let screen = app.clone();
    super::view::shell(&app)
        .theme(Theme::dark())
        .children_reactive(move || {
            drop((
                screen.transcript.get(),
                screen.status.get(),
                screen.layout.get(),
                screen.panes.get(),
                screen.pane_focus.get(),
                screen.diff_view.get(),
                screen.view_edit.get(),
            ));
            vec![
                container()
                    .role(Role::Application)
                    .label("Summoner")
                    .layout(LayoutStyle::flex_row().fill())
                    .class(classes!("bg-gray-1"))
                    .child(rail::rail(screen.clone()))
                    .child(thread::main_col(screen.clone()))
                    .child(side::slot(screen.clone())),
            ]
        })
}

/// Whether `pane` is the one open beside the chat.
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

/// A clickable row or button; `on` paints it selected.
fn hit(name: String, on: bool, act: impl Fn() + 'static) -> NodeBuilder {
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
