use std::path::Path;

use sigil::icons::Icon;
use sigil::prelude::{AlignItems, 
    ControlSize, LayoutStyle, NodeBuilder, Signal, Tone, Widget, button, chip, container, effect, empty_state, icon,
    notice_band, signal,
};
use sigil::reactive::{defer, untrack};

use super::super::App;
use super::super::style::{ICON, L, M, Text, XS, text};
use super::mcp::{actions, item};
use super::parts::section;
use crate::plugins::manage::Plugin;

const DESCRIPTION: usize = 96;

#[derive(Clone, PartialEq)]
struct Installed {
    name: String,
    adds: String,
    on: bool,
}

#[derive(Clone, Copy)]
struct Page {
    version: Signal<u64>,
    armed: Signal<Option<String>>,
    error: Signal<Option<String>>,
    busy: Signal<bool>,
}

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    let page = Page {
        version: signal(0),
        armed: signal(None),
        error: signal(None),
        busy: signal(false),
    };
    let workspace = app.workspace();
    vec![
        container().layout(LayoutStyle::flex_column().fill_width().gap(M).align_items(AlignItems::Stretch)).children_reactive(move || {
            let _ = page.version.get();
            let notice = page.error.get().map(|error| notice_band(error, Tone::Error));
            notice.into_iter().chain([plugins(page, &installed())]).collect::<Vec<_>>()
        }),
        container().layout(LayoutStyle::flex_column().fill_width().align_items(AlignItems::Stretch)).children_reactive(move || {
            let _ = page.version.get();
            vec![skills(&workspace)]
        }),
    ]
}

fn installed() -> Vec<Installed> {
    let on: Vec<String> = crate::plugins::enabled().into_iter().map(|plugin| plugin.name).collect();
    crate::plugins::installed()
        .into_iter()
        .map(|plugin| Installed {
            adds: crate::plugins::manage::report::describe(&plugin).join(" · "),
            on: on.contains(&plugin.name),
            name: plugin.name,
        })
        .collect()
}

fn plugins(page: Page, found: &[Installed]) -> NodeBuilder {
    if found.is_empty() {
        return section(Some("Plugins"), vec![empty_state("No plugins yet", "").child(install_button(page).primary())]);
    }
    let armed = page.armed.get();
    let mut rows: Vec<NodeBuilder> = found
        .iter()
        .map(|plugin| plugin_row(page, plugin, armed.as_deref() == Some(plugin.name.as_str())))
        .collect();
    rows.push(
        container()
            .layout(LayoutStyle::flex_row().fill_width().padding_x(L).padding_y(M))
            .child(install_button(page).ghost()),
    );
    section(Some("Plugins"), rows)
}

fn plugin_row(page: Page, plugin: &Installed, armed: bool) -> NodeBuilder {
    let about = if armed { "Its skills and tools go away" } else { plugin.adds.as_str() };
    let words = container()
        .layout(LayoutStyle::flex_column().gap(XS))
        .child(text(Text::Body, plugin.name.clone()))
        .child(text(Text::Meta, about.to_owned()).wrap());
    let trailing = if armed { confirm(page, &plugin.name) } else { status(page, plugin) };
    item(icon(Icon::Package).px(ICON).into(), words, trailing)
}

fn status(page: Page, plugin: &Installed) -> NodeBuilder {
    let name = plugin.name.clone();
    let row = actions();
    let row = if plugin.on { row } else { row.child(chip("Off", Tone::Neutral)) };
    row.child(
        button("Remove")
            .ghost()
            .tone(Tone::Error)
            .size(ControlSize::Compact)
            .accessible(format!("Remove {}", plugin.name))
            .activate(move || {
                let name = name.clone();
                defer(move || page.armed.set(Some(name)));
            }),
    )
}

fn confirm(page: Page, name: &str) -> NodeBuilder {
    let name = name.to_owned();
    actions()
        .child(
            button("Cancel")
                .ghost()
                .size(ControlSize::Compact)
                .activate(move || defer(move || page.armed.set(None))),
        )
        .child(
            button("Remove")
                .tone(Tone::Error)
                .size(ControlSize::Compact)
                .accessible(format!("Remove {name} now"))
                .activate(move || {
                    let name = name.clone();
                    defer(move || remove(page, &name));
                }),
        )
}

fn remove(page: Page, name: &str) {
    page.armed.set(None);
    match Plugin::remove(name) {
        Ok(_) => page.error.set(None),
        Err(error) => page.error.set(Some(first_line(&error.to_string()))),
    }
    page.version.update(|version| *version += 1);
}

fn install_button(page: Page) -> sigil::widgets::ButtonBuilder {
    button("Install plugin…")
        .icon(Icon::FolderOpen)
        .pending(page.busy)
        .activate(move || choose(page))
}

fn choose(page: Page) {
    let picked = sigil::app::dialog::FileDialog::new().pick_folder();
    page.busy.set(true);
    effect(move || match picked.get() {
        sigil::tasks::Status::Ok(Some(path)) => untrack(|| defer(move || install(page, &path))),
        sigil::tasks::Status::Pending | sigil::tasks::Status::Idle => {}
        _ => untrack(|| page.busy.set(false)),
    });
}

fn install(page: Page, path: &Path) {
    page.busy.set(false);
    match Plugin::install(path) {
        Ok(_) => page.error.set(None),
        Err(error) => page.error.set(Some(first_line(&error.to_string()))),
    }
    page.version.update(|version| *version += 1);
}

fn skills(workspace: &Path) -> NodeBuilder {
    let found = crate::skills::list(workspace);
    if found.is_empty() {
        return section(Some("Skills"), vec![empty_state("No skills yet", "")]);
    }
    let rows = found
        .iter()
        .map(|(name, about)| {
            let words = container()
                .layout(LayoutStyle::flex_column().gap(XS))
                .child(text(Text::Body, name.clone()))
                .child(text(Text::Meta, crate::display::ellipsize(about, DESCRIPTION)));
            item(icon(Icon::ScrollText).px(ICON).into(), words, container().into())
        })
        .collect();
    section(Some("Skills"), rows)
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or(text).to_owned()
}
