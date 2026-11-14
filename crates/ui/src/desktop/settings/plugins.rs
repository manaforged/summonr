use std::path::Path;

use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, ControlSize, NodeBuilder, Signal, Tone, Widget, button, container, effect,
    empty_state, icon, notice_band, segmented, signal, toggle,
};
use sigil::reactive::{defer, untrack};

use super::super::App;
use super::super::style::{ICON, L, M, Text, XS, text};
use super::parts::{actions, confirm_remove, first_line, item, kept_note, section};
use crate::usage::report::EVERY_SKILL;
use summonr_agent::plugins::manage::Plugin;

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
        kept_note(),
        sigil::prelude::column()
            .fill_width()
            .gap(M)
            .align_items(AlignItems::Stretch)
            .children_reactive(move || {
                _ = page.version.get();
                let notice = page
                    .error
                    .get()
                    .map(|error| notice_band(error, Tone::Error));
                notice
                    .into_iter()
                    .chain([plugins(page, &installed())])
                    .collect::<Vec<_>>()
            }),
        sigil::prelude::column()
            .gap(sigil::prelude::Space::None)
            .fill_width()
            .align_items(AlignItems::Stretch)
            .children_reactive(move || {
                _ = page.version.get();
                std::iter::once(sets(page))
                    .chain(skills(&workspace, page))
                    .collect::<Vec<_>>()
            }),
    ]
}

fn installed() -> Vec<Installed> {
    let on: Vec<String> = summonr_agent::plugins::enabled()
        .into_iter()
        .map(|plugin| plugin.name)
        .collect();
    summonr_agent::plugins::installed()
        .into_iter()
        .map(|plugin| Installed {
            adds: summonr_agent::plugins::manage::report::describe(&plugin).join(" · "),
            on: on.contains(&plugin.name),
            name: plugin.name,
        })
        .collect()
}

fn plugins(page: Page, found: &[Installed]) -> NodeBuilder {
    if found.is_empty() {
        return section(
            Some("Plugins"),
            vec![empty_state("No plugins yet", "").child(install_button(page).primary())],
        );
    }
    let armed = page.armed.get();
    let mut rows: Vec<NodeBuilder> = found
        .iter()
        .map(|plugin| plugin_row(page, plugin, armed.as_deref() == Some(plugin.name.as_str())))
        .collect();
    rows.push(
        sigil::prelude::row()
            .gap(sigil::prelude::Space::None)
            .align_items(sigil::prelude::AlignItems::Stretch)
            .fill_width()
            .padding_x(L)
            .padding_y(M)
            .child(install_button(page).ghost()),
    );
    section(Some("Plugins"), rows)
}

fn plugin_row(page: Page, plugin: &Installed, armed: bool) -> NodeBuilder {
    let about = if armed {
        "Its skills and tools go away"
    } else {
        plugin.adds.as_str()
    };
    let words = sigil::prelude::column()
        .gap(XS)
        .child(text(Text::Body, plugin.name.clone()))
        .child(text(Text::Meta, about.to_owned()).wrap());
    let trailing = if armed {
        confirm(page, &plugin.name)
    } else {
        status(page, plugin)
    };
    item(icon(Icon::Package).px(ICON).into(), words, trailing)
}

fn status(page: Page, plugin: &Installed) -> NodeBuilder {
    let name = plugin.name.clone();
    let flip = plugin.name.clone();
    actions()
        .child(
            toggle(signal(plugin.on))
                .accessible(format!("Use {}", plugin.name))
                .change(move |on| {
                    let name = flip.clone();
                    defer(move || set_on(page, &name, on));
                }),
        )
        .child(
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

fn set_on(page: Page, name: &str, on: bool) {
    match Plugin::set_on(name, on) {
        Ok(()) => page.error.set(None),
        Err(error) => page.error.set(Some(first_line(&error.to_string()))),
    }
    page.version.update(|version| *version += 1);
}

fn confirm(page: Page, name: &str) -> NodeBuilder {
    let target = name.to_owned();
    confirm_remove(
        name,
        move || defer(move || page.armed.set(None)),
        move || {
            let target = target.clone();
            defer(move || remove(page, &target));
        },
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
        sigil::tasks::Status::Ok(None)
        | sigil::tasks::Status::Err(_)
        | sigil::tasks::Status::Panicked(_) => untrack(|| page.busy.set(false)),
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

/// The set new agents start with, written to `[skills] default` when picked.
fn sets(page: Page) -> NodeBuilder {
    let names = match summonr_agent::skills::set_names() {
        Ok(names) => names,
        Err(error) => return notice_band(first_line(&error.to_string()), Tone::Error),
    };
    let current = summonr_agent::skills::default_set().ok().flatten();
    let at = current
        .and_then(|current| names.iter().position(|name| *name == current))
        .map_or(0, |index| index + 1);
    let labels = std::iter::once(EVERY_SKILL.to_owned())
        .chain(names.iter().cloned())
        .collect();
    let active = signal(at);
    effect(move || {
        let chosen = active
            .get()
            .checked_sub(1)
            .and_then(|index| names.get(index))
            .cloned();
        let saved = untrack(|| summonr_agent::skills::default_set().ok().flatten());
        if chosen != saved
            && let Err(error) = summonr_agent::skills::choose_default(chosen.as_deref())
        {
            page.error.set(Some(first_line(&error.to_string())));
        }
    });
    section(
        Some("Skill set for new agents"),
        vec![segmented(active, labels).label("Skill set").into()],
    )
}

/// One section per skill folder, skills at a root first.
fn skills(workspace: &Path, page: Page) -> Vec<NodeBuilder> {
    let folders = summonr_agent::skills::by_folder(workspace);
    if folders.is_empty() {
        return vec![section(
            Some("Skills"),
            vec![empty_state("No skills yet", "")],
        )];
    }
    folders
        .iter()
        .map(|(folder, found)| {
            let title = if folder.is_empty() {
                "Skills".to_owned()
            } else {
                format!("Skills · {folder}")
            };
            let rows = found
                .iter()
                .map(|(name, about)| skill(workspace, name, about, page))
                .collect();
            section(Some(&title), rows)
        })
        .collect()
}

fn skill(workspace: &Path, name: &str, about: &str, page: Page) -> NodeBuilder {
    let words = sigil::prelude::column()
        .gap(XS)
        .child(text(Text::Body, name.to_owned()))
        .child(text(
            Text::Meta,
            crate::cells::ellipsize(about, DESCRIPTION),
        ));
    item(
        icon(Icon::ScrollText).px(ICON).into(),
        words,
        open_skill(workspace, name, page),
    )
}

fn open_skill(workspace: &Path, name: &str, page: Page) -> NodeBuilder {
    let Some(file) = summonr_agent::skills::file(workspace, name) else {
        return container();
    };
    button("Open")
        .ghost()
        .size(ControlSize::Compact)
        .accessible(format!("Open {name}"))
        .activate(move || {
            if let Err(error) = summonr_auth::open::external(&file) {
                page.error.set(Some(first_line(&error.to_string())));
            }
        })
        .into()
}
