use std::cell::RefCell;
use std::rc::Rc;

use sigil::prelude::{
    IntoNode, LayoutStyle, NodeBuilder, Paint, Signal, container, effect, modal, signal,
    virtual_list,
};
use sigil::reactive::{defer, untrack};

use crate::prefs::Hotkeys;
use crate::tui::sigil_view::canvas;

use super::hints::Hints;
use super::{List, filter_column, overlay_list, plain};
use sigil::prelude::Widget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) enum Size {
    Normal,
    Reading,
    Page,
}

impl Size {
    pub(in crate::tui) fn width(self, viewport: (u16, u16)) -> f32 {
        match self {
            Self::Normal | Self::Reading | Self::Page if canvas::is_window() => {
                canvas::window_frame(viewport).0
            }
            Self::Normal | Self::Reading | Self::Page => {
                f32::from(viewport.0.saturating_sub(2)) * canvas::cell().w
            }
        }
    }

    #[must_use]
    pub(in crate::tui) fn cells(self, viewport: (u16, u16)) -> usize {
        (self.width(viewport) / canvas::cell().w) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::Size;

    #[test]
    fn a_menu_uses_the_terminal_width() {
        assert!(
            Size::Normal.cells((140, 40)) >= 130,
            "{}",
            Size::Normal.cells((140, 40))
        );
        assert!(Size::Reading.cells((140, 40)) >= 130);
        assert!(Size::Page.cells((80, 24)) >= 70);
    }
}

fn body_layout() -> LayoutStyle {
    LayoutStyle::flex_column()
        .fill_width()
        .padding_x(8.0)
        .padding_y(canvas::overlay_gap() * 4.0)
        .gap(canvas::overlay_gap())
}

type Dismiss = Rc<dyn Fn()>;

thread_local! {
    static DISMISS: RefCell<Option<Dismiss>> = const { RefCell::new(None) };
}

pub(in crate::tui::sigil_view) fn dismissing<T>(close: Dismiss, build: impl FnOnce() -> T) -> T {
    let previous = DISMISS.with(|slot| slot.replace(Some(close)));
    let built = build();
    DISMISS.with(|slot| *slot.borrow_mut() = previous);
    built
}

fn backdrop_signal(open: Signal<bool>, close: Dismiss) -> Signal<bool> {
    let shown = signal(open.peek());
    effect(move || {
        let now = open.get();
        if shown.peek() != now {
            shown.set(now);
        }
    });
    effect(move || {
        if !shown.get() && open.peek() {
            untrack(|| close());
            defer(move || {
                if open.peek() && !shown.peek() {
                    shown.set(true);
                }
            });
        }
    });
    shown
}

pub(in crate::tui) struct Dialog {
    size: Size,
    title: String,
    header: Option<NodeBuilder>,
    rows: Option<u16>,
    body: NodeBuilder,
    hints: Rc<dyn Fn() -> Hints>,
}

impl Dialog {
    #[must_use]
    pub(in crate::tui) fn new(size: Size, title: impl Into<String>, hints: Hints) -> Self {
        Self {
            size,
            title: title.into(),
            header: None,
            rows: None,
            body: container().layout(body_layout()),
            hints: Rc::new(move || hints.clone()),
        }
    }

    #[must_use]
    pub(in crate::tui) fn live_hints(mut self, hints: impl Fn() -> Hints + 'static) -> Self {
        self.hints = Rc::new(hints);
        self
    }

    #[must_use]
    pub(in crate::tui) fn header(mut self, header: Option<NodeBuilder>) -> Self {
        self.header = header;
        self
    }

    #[must_use]
    pub(in crate::tui) fn fixed(mut self, rows: u16) -> Self {
        self.rows = (!canvas::is_window()).then_some(rows);
        self
    }

    #[must_use]
    pub(in crate::tui) fn child(mut self, node: impl IntoNode + 'static) -> Self {
        self.body = self.body.child(node);
        self
    }

    #[must_use]
    pub(in crate::tui) fn collection<T: 'static>(
        self,
        list: List,
        rows: impl Fn() -> Result<Vec<T>, String> + 'static,
        empty: impl Fn() -> String + 'static,
        viewport: impl Fn(usize) -> Option<(f32, usize)> + 'static,
        render: impl Fn(&T, bool) -> NodeBuilder + Clone + 'static,
        activate: impl Fn() + Clone + 'static,
    ) -> Self {
        self.child(filter_column().children_reactive(move || {
            let rows = match rows() {
                Ok(rows) => rows,
                Err(error) => return vec![plain(error)],
            };
            let cursor = list.cursor.get();
            let selected = cursor.min(rows.len().saturating_sub(1));
            if cursor != selected {
                defer(move || list.select(selected));
            }
            let viewport = viewport(rows.len());
            let mut children = Vec::new();
            if rows.is_empty() {
                children.push(plain(empty()));
            }
            if let Some((height, shown)) = viewport {
                let render = render.clone();
                let activate = activate.clone();
                children.push(overlay_list(
                    virtual_list(rows, move |index, item: &T| {
                        let activate = activate.clone();
                        render(item, index == selected).pointer_down(move |event| {
                            list.select(index);
                            activate();
                            event.stop_propagation();
                        })
                    })
                    .follow_row(list.cursor),
                    height,
                    shown,
                ));
            }
            children
        }))
    }

    #[must_use]
    pub(in crate::tui) fn build(
        self,
        open: Signal<bool>,
        viewport: (u16, u16),
        keys: &Hotkeys,
    ) -> NodeBuilder {
        let width = self.size.width(viewport);
        let hints = self.hints;
        let keys = keys.clone();
        let bar = container()
            .layout(LayoutStyle::flex_column().fill_width())
            .children_reactive(move || vec![hints().bar(&keys)]);
        let body = match self.rows {
            Some(rows) => container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .height(f32::from(canvas::page_rows(rows)) * canvas::cell().h),
                )
                .child(self.body)
                .child(container().layout(LayoutStyle::flex_column().grow(1.0)))
                .child(bar),
            None => self.body.child(bar),
        };
        let dismiss = DISMISS.with(|slot| slot.borrow().clone());
        let backdrop = dismiss.is_some();
        let shown = dismiss.map_or(open, |close| backdrop_signal(open, close));
        let mut shell = modal(shown)
            .trap(true)
            .dismiss_on_backdrop(backdrop)
            .escape_closes(!backdrop)
            .width(width)
            .border_w(canvas::frame_border());
        if canvas::is_window() {
            shell = shell.height(canvas::window_frame(viewport).1);
        }
        let shell = match self.header {
            Some(header) => shell.child(
                container()
                    .layout(LayoutStyle::flex_column().fill_width())
                    .child(header)
                    .child(body.rounded(sigil::prelude::Radius::Base)),
            ),
            None => shell
                .title(self.title)
                .child(body.rounded(sigil::prelude::Radius::Base)),
        };
        container().layout(LayoutStyle::block()).child(shell)
    }
}
