use std::cell::RefCell;
use std::rc::Rc;

use sigil::prelude::{
    IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, Paint, Signal, WidgetLayoutExt,
    container, effect, modal, signal,
};
use sigil::reactive::untrack;

use crate::prefs::Hotkeys;
use crate::tui::sigil_view::canvas;

use super::hints::Hints;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) enum Size {
    Normal,
    Reading,
    Page,
}

impl Size {
    pub(in crate::tui) fn width(self, cols: u16) -> f32 {
        if canvas::is_window() {
            let window = f32::from(cols) * canvas::cell().w;
            return match self {
                Self::Normal => canvas::WINDOW_DIALOG_W,
                Self::Reading | Self::Page => (window * canvas::WINDOW_PAGE_FRACTION)
                    .clamp(canvas::WINDOW_DIALOG_W, canvas::WINDOW_PAGE_MAX_W),
            };
        }
        let max_cells = match self {
            Self::Normal | Self::Reading => canvas::DIALOG_CELLS,
            Self::Page => f32::from(cols),
        };
        (f32::from(cols.saturating_sub(4)) * canvas::cell().w).min(max_cells * canvas::cell().w)
    }

    #[must_use]
    pub(in crate::tui) fn cells(self, cols: u16) -> usize {
        (self.width(cols) / canvas::cell().w) as usize
    }
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
            if open.peek() {
                shown.set(true);
            }
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
            body: container().layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .padding_x(8.0)
                    .padding_y(canvas::overlay_gap() * 4.0)
                    .gap(canvas::overlay_gap()),
            ),
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
    pub(in crate::tui) fn build(
        self,
        open: Signal<bool>,
        cols: u16,
        keys: &Hotkeys,
    ) -> NodeBuilder {
        let width = self.size.width(cols);
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
        let dismiss = DISMISS
            .with(|slot| slot.borrow().clone())
            .filter(|_| canvas::is_window());
        let backdrop = dismiss.is_some();
        let shown = dismiss.map_or(open, |close| backdrop_signal(open, close));
        let shell = modal(shown)
            .trap(true)
            .dismiss_on_backdrop(backdrop)
            .escape_closes(!backdrop)
            .width(width)
            .border_w(canvas::frame_border());
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
