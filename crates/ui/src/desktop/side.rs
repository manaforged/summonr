use sigil::icons::Icon;
use sigil::prelude::{AlignItems, ControlSize, NodeBuilder, Paint, Widget, button};

use super::style::{DIVIDER, S, XS};
use super::{App, Pane};

/// The tabs of the right panel, in order.
pub(super) const TABS: [(Pane, &str); 3] = [
    (Pane::Diff, "Changes"),
    (Pane::Files, "Files"),
    (Pane::Terminal, "Terminal"),
];

/// The strip's right side: the diff stat that opens Changes, then the Terminal toggle.
pub(super) fn toggles(app: &App) -> Vec<NodeBuilder> {
    let (watch, terminal) = (app.clone(), app.clone());
    let stat = sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .align_items(sigil::prelude::AlignItems::Stretch)
        .shrink(0.0)
        .children_reactive(move || {
            let model = watch.model.get();
            if model.diff.files.is_empty() || model.entries.only_notes().is_some() {
                return Vec::new();
            }
            let open = watch.clone();
            vec![NodeBuilder::from(
                button(format!("+{} −{}", model.diff.added(), model.diff.removed()))
                    .ghost()
                    .size(ControlSize::Compact)
                    .accessible("Changes")
                    .activate(move || open.toggle_pane(Pane::Diff)),
            )]
        });
    let shell = sigil::prelude::button(Icon::Terminal)
        .activate(move || terminal.toggle_pane(Pane::Terminal))
        .accessible(TERMINAL);
    vec![stat, super::tip(TERMINAL, Some("⌃`"), shell.into())]
}

const TERMINAL: &str = "Terminal";

/// The right panel: its tabs above the open pane.
pub(super) fn column(app: &App, pane: Pane, cols: u16) -> NodeBuilder {
    let width = f32::from(cols) * crate::canvas::cell().w;
    let body = match pane {
        Pane::Terminal => super::terminal::view(app, width),
        Pane::Custom => super::panel::view(app),
        Pane::Chat
        | Pane::Ledger
        | Pane::Files
        | Pane::Diff
        | Pane::Worktrees
        | Pane::Jobs
        | Pane::Mcp
        | Pane::Peers
        | Pane::View => crate::panes::body(app, pane, cols),
    };
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_height()
        .width(width)
        .min_width(0.0)
        .min_height(0.0)
        .background(super::style::RECESSED)
        .child(tabs(app, pane))
        .child(
            sigil::prelude::column()
                .gap(sigil::prelude::Space::None)
                .grow(1.0)
                .min_width(0.0)
                .min_height(0.0)
                .child(body),
        )
}

fn tabs(app: &App, pane: Pane) -> NodeBuilder {
    let close = app.clone();
    TABS.iter()
        .fold(
            sigil::prelude::row()
                .fill_width()
                .height(super::sidebar::BAR)
                .shrink(0.0)
                .padding_x(S)
                .gap(XS)
                .align_items(AlignItems::Center),
            |row, (tab, label)| row.child(tab_button(app, *tab, label, *tab == pane)),
        )
        .child(super::panel::tabs(app, pane))
        .border_bottom(1)
        .border_color(DIVIDER)
        .child(super::drag_area())
        .child(
            sigil::prelude::button(Icon::X)
                .activate(move || close.toggle_side())
                .accessible("Close panel"),
        )
}

fn tab_button(app: &App, tab: Pane, label: &str, current: bool) -> NodeBuilder {
    let open = app.clone();
    let tab_button = button(label).size(ControlSize::Compact).activate(move || {
        if open.layout.peek().secondary != Some(tab) {
            open.toggle_pane(tab);
        }
    });
    if current {
        tab_button.soft()
    } else {
        tab_button.ghost()
    }
    .into()
}
