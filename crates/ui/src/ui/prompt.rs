use std::rc::Rc;

use sigil::prelude::{
    AlignItems, LayoutStyle, NodeBuilder, Paint, Tone, button, container, notice_band,
};

use summonr_agent::prefs::Hotkeys;

use super::hints::{Hints, Key};
use sigil::prelude::Widget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActionKind {
    Primary,
    Plain,
    Destructive,
}

#[derive(Clone)]
pub(crate) struct Action {
    label: &'static str,
    key: Key,
    kind: ActionKind,
    run: Rc<dyn Fn()>,
}

impl Action {
    #[must_use]
    pub(crate) fn new(
        label: &'static str,
        key: Key,
        kind: ActionKind,
        run: impl Fn() + 'static,
    ) -> Self {
        Self {
            label,
            key,
            kind,
            run: Rc::new(run),
        }
    }

    fn node(self, keys: &Hotkeys) -> impl sigil::prelude::IntoNode + 'static {
        let run = self.run;
        let base = button(format!("{} ({})", self.label, self.key.label(keys)))
            .accessible(self.label.to_owned());
        let styled = match self.kind {
            ActionKind::Primary => base.primary(),
            ActionKind::Plain => base,
            ActionKind::Destructive => base.destructive(),
        };
        styled.activate(move || run())
    }
}

pub(crate) struct Prompt {
    tone: Tone,
    head: String,
    lines: Vec<String>,
    hints: Hints,
    actions: Vec<Action>,
}

impl Prompt {
    #[must_use]
    pub(crate) fn new(tone: Tone, head: impl Into<String>, hints: Hints) -> Self {
        Self {
            tone,
            head: head.into(),
            lines: Vec::new(),
            hints,
            actions: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) fn line(mut self, text: impl Into<String>) -> Self {
        self.lines.push(text.into());
        self
    }

    #[must_use]
    pub(crate) fn action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }

    #[must_use]
    pub(crate) fn build(self, width: usize, keys: &Hotkeys) -> NodeBuilder {
        let fit = |text: &str| crate::cells::ellipsize(text, width.saturating_sub(4));
        let mut band = container()
            .layout(LayoutStyle::flex_column().fill_width().gap(8.0))
            .child(notice_band(fit(&self.head), self.tone));
        let taken: Vec<Key> = self.actions.iter().map(|action| action.key).collect();
        let hints = self.hints.without(&taken).text(keys);
        for line in self
            .lines
            .iter()
            .chain((!hints.is_empty()).then_some(&hints))
        {
            band = band.child(
                sigil::prelude::text(fit(line))
                    .nowrap()
                    .text_color(sigil::prelude::Token::OnSurface),
            );
        }
        if self.actions.is_empty() {
            return band;
        }
        let actions = self.actions.into_iter().fold(
            container().layout(
                LayoutStyle::flex_row()
                    .gap(12.0)
                    .align_items(AlignItems::Center),
            ),
            |row, action| row.child(action.node(keys)),
        );
        band.child(actions)
    }
}
