use std::rc::Rc;

use sigil::prelude::{
    IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, Paint, Signal, WidgetLayoutExt,
    container, modal,
};

use crate::prefs::Hotkeys;
use crate::tui::sigil_view::canvas;

use super::hints::Hints;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) enum Size {
    Normal,
    Page,
}

impl Size {
    fn width(self, cols: u16) -> f32 {
        let cells = match self {
            Self::Normal => canvas::DIALOG_CELLS,
            Self::Page => f32::from(cols),
        };
        canvas::dialog_width(cols, cells)
    }
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
        let shell = modal(open)
            .trap(true)
            .width(width)
            .border_w(canvas::cell().h as i16);
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
