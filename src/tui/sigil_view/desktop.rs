use sigil::prelude::Paint;
use sigil::prelude::{LayoutStyle, NodeBuilder, Theme, Token, container};

use sigil::events::ScrollEvent;
use sigil::layout::Position;
use sigil::prelude::{Signal, scroll};

use super::view::shell;
use crate::tui::Cmd;

pub(super) use super::app::App;
pub(super) use super::chat;
pub(super) use super::pane::Pane;

pub(super) mod entry;
mod icons;
mod rail;
mod thread;

pub(super) fn root(app: App) -> NodeBuilder {
    drop(app.cmd.send(Cmd::Diff(None)));
    let side = app.clone();
    let overlays = app.clone();
    let pane_scroll = sigil::reactive::signal(0.0_f32);
    let work = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .grow(1.0)
                .min_size(0.0, 0.0),
        )
        .child(rail::rail(app.clone()))
        .child(thread::main_col(app.clone()))
        .child(
            container()
                .layout(LayoutStyle::block().fill_height().shrink(0.0))
                .children_reactive(move || vec![pane(side.clone(), pane_scroll)]),
        );
    let window = container()
        .layout(LayoutStyle::flex_column().fill())
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().shrink(0.0))
                .bg(Token::SurfaceSubtle)
                .border_bottom(1)
                .border_color(Token::Border)
                .child(thread::title_bar(app.clone())),
        )
        .child(work)
        .children_reactive(move || {
            let _viewport = overlays.viewport.get();
            overlays
                .overlay()
                .and_then(|overlay| overlay.layer(overlays.clone()))
                .map(|layer| {
                    container()
                        .layout(
                            LayoutStyle::flex_column()
                                .position(Position::Absolute)
                                .fill(),
                        )
                        .child(layer)
                })
        });
    shell(&app).theme(Theme::dark()).child(window)
}

const PANE_SHARE: f32 = 0.4;
const PANE_MIN: f32 = 360.0;
const PANE_CAP: f32 = 0.62;

fn pane(app: App, offset: Signal<f32>) -> NodeBuilder {
    let Some(pane) = app.layout.get().secondary else {
        return closed();
    };
    let window = f32::from(app.viewport.get().0) * super::canvas::cell().w;
    let width = pane_width(window);
    let focus = app.clone();
    let focused = app.pane_focus.get();
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

fn pane_width(window: f32) -> f32 {
    let share = window * PANE_SHARE;
    let cap = (window * PANE_CAP).max(PANE_MIN);
    share.clamp(PANE_MIN, cap)
}

#[cfg(test)]
mod pane_tests {
    use super::pane_width;

    #[test]
    fn a_side_pane_grows_with_the_window() {
        assert!((pane_width(1280.0) - 512.0).abs() < 1.0);
        assert!(pane_width(2400.0) > 760.0, "{}", pane_width(2400.0));
    }
}

fn closed() -> NodeBuilder {
    container().layout(LayoutStyle::block().fixed_size(0.0, 0.0).shrink(0.0))
}

fn open(app: &App, pane: Pane) -> bool {
    app.layout.get().secondary == Some(pane)
}

#[cfg(test)]
mod tests;
