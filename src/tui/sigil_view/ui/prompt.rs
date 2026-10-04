use std::rc::Rc;

use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, NodeBuilder, Paint, Tone, button, container,
    notice_band,
};

use crate::prefs::Hotkeys;

use super::hints::Hints;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) enum ActionKind {
    Primary,
    Plain,
    Destructive,
}

#[derive(Clone)]
pub(in crate::tui) struct Action {
    label: &'static str,
    kind: ActionKind,
    run: Rc<dyn Fn()>,
}

impl Action {
    #[must_use]
    pub(in crate::tui) fn new(
        label: &'static str,
        kind: ActionKind,
        run: impl Fn() + 'static,
    ) -> Self {
        Self {
            label,
            kind,
            run: Rc::new(run),
        }
    }

    fn node(self) -> impl sigil::prelude::IntoNode + 'static {
        let run = self.run;
        let base = button(self.label);
        let styled = match self.kind {
            ActionKind::Primary => base.primary(),
            ActionKind::Plain => base,
            ActionKind::Destructive => base.destructive(),
        };
        styled.activate(move || run())
    }
}

pub(in crate::tui) struct Prompt {
    tone: Tone,
    head: String,
    lines: Vec<String>,
    hints: Hints,
    actions: Vec<Action>,
}

impl Prompt {
    #[must_use]
    pub(in crate::tui) fn new(tone: Tone, head: impl Into<String>, hints: Hints) -> Self {
        Self {
            tone,
            head: head.into(),
            lines: Vec::new(),
            hints,
            actions: Vec::new(),
        }
    }

    #[must_use]
    pub(in crate::tui) fn line(mut self, text: impl Into<String>) -> Self {
        self.lines.push(text.into());
        self
    }

    #[must_use]
    pub(in crate::tui) fn action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }

    #[must_use]
    pub(in crate::tui) fn build(self, width: usize, keys: &Hotkeys) -> NodeBuilder {
        let fit = |text: &str| crate::text::ellipsize(text, width.saturating_sub(4));
        let mut band = container()
            .layout(LayoutStyle::flex_column().fill_width().gap(8.0))
            .child(notice_band(fit(&self.head), self.tone));
        for line in &self.lines {
            band = band.child(
                sigil::kernel::text(fit(line))
                    .nowrap()
                    .text_color(sigil::prelude::Token::OnSurface),
            );
        }
        band = band.child(
            sigil::kernel::text(fit(&self.hints.text(keys)))
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface),
        );
        let actions = self.actions.into_iter().fold(
            container().layout(
                LayoutStyle::flex_row()
                    .gap(12.0)
                    .align_items(AlignItems::Center),
            ),
            |row, action| row.child(action.node()),
        );
        band.child(actions)
    }
}
