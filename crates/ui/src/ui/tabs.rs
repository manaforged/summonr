use sigil::prelude::Space;
use sigil::prelude::{AlignItems, NodeBuilder, Token, effect, signal, tabs};

use crate::app::App;
use crate::canvas;
use crate::text_style::TextStyle;

use sigil::prelude::Widget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
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
    pub(crate) fn step(self, delta: isize) -> Self {
        let at = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[super::cycle(at, delta, Self::ALL.len())]
    }
}

pub(crate) fn strip(app: &App, active: Tab) -> NodeBuilder {
    let at = Tab::ALL.iter().position(|tab| *tab == active).unwrap_or(0);
    let picked = signal(at);
    let show = app.clone();
    effect(move || {
        let index = picked.get();
        if index != at {
            let show = show.clone();
            sigil::reactive::defer(move || show.show_tab(Tab::ALL[index]));
        }
    });
    let labels: Vec<String> = Tab::ALL.iter().map(|tab| tab.label().to_string()).collect();
    let line = sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .min_width(0.0)
        .min_height(canvas::overlay_list_row())
        .padding_x(Space::Sm)
        .align_items(AlignItems::Center)
        .child(tabs().labels(labels).active(picked).label("Settings tabs"));
    if canvas::is_window() {
        return line;
    }
    line.child(TextStyle::xs(Token::Gray9).on(sigil::prelude::text(TAB_HINT).nowrap()))
}

const TAB_HINT: &str = "←/→";
