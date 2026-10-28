use sigil::prelude::{
    AlignItems, LayoutBuilderExt, NodeBuilder, Token, container, effect, signal, tabs,
};

use crate::tui::sigil_view::app::App;
use crate::tui::sigil_view::canvas;
use crate::tui::sigil_view::text_style::TextStyle;

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
        Self::ALL[super::cycle(at, delta, Self::ALL.len())]
    }
}

pub(in crate::tui::sigil_view) fn strip(app: &App, active: Tab) -> NodeBuilder {
    let at = Tab::ALL.iter().position(|tab| *tab == active).unwrap_or(0);
    let picked = signal(at);
    let show = app.clone();
    effect(move || {
        let index = picked.get();
        if index != at {
            show.show_tab(Tab::ALL[index]);
        }
    });
    let labels: Vec<String> = Tab::ALL.iter().map(|tab| tab.label().to_string()).collect();
    let line = container()
        .layout(row(8.0).align_items(AlignItems::Center))
        .child(tabs().labels(labels).active(picked).label("Settings tabs"));
    if canvas::is_window() {
        return line;
    }
    line.child(TextStyle::xs(Token::Gray9).on(sigil::kernel::text(TAB_HINT).nowrap()))
}

const TAB_HINT: &str = "←/→";
