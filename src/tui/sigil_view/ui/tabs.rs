use sigil::prelude::{AlignItems, LayoutBuilderExt, NodeBuilder, NodeEventsExt, Token, container};

use crate::tui::sigil_view::app::App;
use crate::tui::sigil_view::canvas;
use crate::tui::sigil_view::look::Look;

use super::rows::row;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) enum Tab {
    Models,
    Providers,
    Usage,
}

impl Tab {
    const ALL: [Self; 3] = [Self::Models, Self::Providers, Self::Usage];

    const fn label(self) -> &'static str {
        match self {
            Self::Models => "Models",
            Self::Providers => "Providers",
            Self::Usage => "Usage",
        }
    }

    #[must_use]
    pub(in crate::tui) fn step(self, delta: isize) -> Self {
        let at = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        let next = (at as isize + delta).rem_euclid(Self::ALL.len() as isize);
        Self::ALL[next as usize]
    }
}

pub(in crate::tui::sigil_view) fn strip(app: &App, active: Tab) -> NodeBuilder {
    let line = container().layout(
        row(8.0)
            .align_items(AlignItems::Center)
            .gap(canvas::cell().w * 3.0),
    );
    Tab::ALL
        .into_iter()
        .fold(line, |line, tab| {
            let look = if tab == active {
                Look::sm(Token::Accent11).bold()
            } else {
                Look::sm(Token::Gray10)
            };
            let app = app.clone();
            line.child(
                container()
                    .child(look.on(sigil::kernel::text(tab.label()).nowrap()))
                    .pointer_down(move |event| {
                        app.show_tab(tab);
                        event.stop_propagation();
                    }),
            )
        })
        .child(Look::xs(Token::Gray9).on(sigil::kernel::text("←/→").nowrap()))
}
