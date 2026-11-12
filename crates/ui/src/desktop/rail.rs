use sigil::icons::Icon;
use sigil::prelude::{NodeBuilder, Paint, Widget, button, container, derived};
use summonr_agent::panel::{Panel, Place};

use super::App;
use super::style::{DIVIDER, RECESSED, XS};

/// The rail button that shows the threads.
pub(super) const THREADS: &str = "Threads";
const WIDTH: f32 = 44.0;
/// The icon of a side panel whose file names none, or a name the app does not bundle.
const PANEL: Icon = Icon::LayoutDashboard;

/// The icon rail beside the sidebar, shown while a panel sits there: the threads, then each side panel.
pub(super) fn view(app: &App) -> NodeBuilder {
    let panels = app.panels;
    let side = derived(move || {
        panels
            .list
            .get()
            .into_iter()
            .filter(|panel| panel.place == Place::Side)
            .collect::<Vec<Panel>>()
    });
    let app = app.clone();
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_height()
        .shrink(0.0)
        .children_reactive(move || -> Vec<NodeBuilder> {
            let side = side.get();
            if side.is_empty() {
                return Vec::new();
            }
            let picked = panels
                .side
                .get()
                .filter(|name| side.iter().any(|panel| &panel.name == name));
            let threads = entry(&app, None, Icon::House, THREADS, picked.is_none());
            let rest = side.into_iter().map(|panel| {
                let current = picked.as_deref() == Some(panel.name.as_str());
                entry(
                    &app,
                    Some(panel.name),
                    icon(panel.icon.as_deref()),
                    &panel.title,
                    current,
                )
            });
            vec![
                std::iter::once(threads)
                    .chain(rest)
                    .fold(
                        sigil::prelude::column()
                            .child(container().height(super::sidebar::BAR).shrink(0.0))
                            .fill_height()
                            .width(WIDTH)
                            .gap(XS)
                            .align_items(sigil::prelude::AlignItems::Center),
                        |rail, button| rail.child(button),
                    )
                    .background(RECESSED)
                    .border_right(1)
                    .border_color(DIVIDER)
                    .into(),
            ]
        })
}

fn entry(app: &App, name: Option<String>, glyph: Icon, label: &str, current: bool) -> NodeBuilder {
    let pick = app.clone();
    let button =
        button(glyph).activate(move || pick.panels.side.set(name.clone())).accessible(label.to_owned());
    super::tip(
        label,
        None,
        if current { button.soft() } else { button }.into(),
    )
}

/// The bundled Lucide icon a panel file names.
fn icon(name: Option<&str>) -> Icon {
    name.and_then(|name| Icon::ALL.iter().copied().find(|icon| icon.name() == name))
        .unwrap_or(PANEL)
}
