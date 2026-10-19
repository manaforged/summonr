use sigil::prelude::Paint;
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Theme, Token, container,
};

use sigil::events::ScrollEvent;
use sigil::layout::Position;
use sigil::prelude::{NodeEventsExt, Signal, scroll};

use super::view::shell;
use crate::tui::Cmd;

pub(super) use super::app::App;
pub(super) use super::chat;
pub(super) use super::pane::Pane;

pub(super) mod entry;
mod icons;
mod rail;
#[cfg(test)]
mod tests;
mod thread;

pub(super) fn root(app: App) -> NodeBuilder {
    drop(app.cmd.send(Cmd::Diff(None)));
    let screen = app.clone();
    let pane_scroll = sigil::reactive::signal(0.0_f32);
    shell(&screen)
        .theme(Theme::dark())
        .children_reactive(move || {
            let chrome = screen.clone();
            let navigation = screen.clone();
            let side = screen.clone();
            let top = screen.clone();
            let work = container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .grow(1.0)
                        .min_size(0.0, 0.0),
                )
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
                                side.viewport.get(),
                            ));
                            vec![pane(side.clone(), pane_scroll)]
                        }),
                );
            let mut window = container()
                .layout(LayoutStyle::flex_column().fill())
                .child(
                    container()
                        .layout(LayoutStyle::block().fill_width().shrink(0.0))
                        .bg(Token::SurfaceSubtle)
                        .border_bottom(1)
                        .border_color(Token::Border)
                        .children_reactive(move || {
                            let _updates = top.layout.get();
                            vec![thread::title_bar(top.clone())]
                        }),
                )
                .child(work);
            if let Some(layer) = overlay(&screen) {
                window = window.child(
                    container()
                        .layout(
                            LayoutStyle::flex_column()
                                .position(Position::Absolute)
                                .fill(),
                        )
                        .child(layer),
                );
            }
            vec![window]
        })
}

fn overlay(app: &App) -> Option<NodeBuilder> {
    app.overlay().and_then(|overlay| overlay.layer(app.clone()))
}

const PANE_SHARE: f32 = 0.34;
const PANE_MIN: f32 = 360.0;
const PANE_MAX: f32 = 760.0;

fn pane(app: App, offset: Signal<f32>) -> NodeBuilder {
    let Some(pane) = app.layout.peek().secondary else {
        return closed();
    };
    let window = f32::from(app.viewport.peek().0) * super::canvas::cell().w;
    let width = (window * PANE_SHARE).clamp(PANE_MIN, PANE_MAX);
    let focus = app.clone();
    let focused = app.pane_focus.peek();
    let content = super::panes::body::pane_body(app, pane, width);
    container()
        .layout(
            LayoutStyle::flex_column()
                .width(width)
                .fill_height()
                .shrink(0.0),
        )
        .bg(Token::Gray3)
        .border_left(1)
        .border_color(if focused {
            Token::Accent
        } else {
            Token::Border
        })
        .pointer_down_capture(move |_| {
            if !focus.pane_focus.peek() {
                focus.focus_pane(true);
            }
        })
        .on_scroll(ScrollEvent::stop_propagation)
        .child(scroll(content).fill().scroll_bind(offset))
}

fn closed() -> NodeBuilder {
    container().layout(LayoutStyle::block().fixed_size(0.0, 0.0).shrink(0.0))
}

fn open(app: &App, pane: Pane) -> bool {
    app.layout.peek().secondary == Some(pane)
}
