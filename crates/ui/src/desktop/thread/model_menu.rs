use sigil::prelude::{ControlSize, NodeBuilder, Signal, Widget, button};
use sigil::widgets::{MenuItem, menu};

use super::super::friends::{Seen, State};
use super::App;
use summonr_agent::endpoint::Friend;
use summonr_agent::recent::ModelId;

const ROWS_MAX: usize = 10;
const CURRENT: &str = "✓";

/// The model chip, with its effort when the model has levels; it opens the menu above the composer.
pub(super) fn chip(app: &App, open: Signal<bool>) -> NodeBuilder {
    let model = app.model;
    button(move || {
        let snapshot = &model.get().snapshot;
        let name = title(&snapshot.endpoint, &snapshot.model);
        match snapshot.effort.filter(|_| !snapshot.levels.is_empty()) {
            Some(effort) => format!("{name} · {}", effort.label()),
            None => name,
        }
    })
    .ghost()
    .trailing(sigil::icons::Icon::ChevronDown)
    .size(ControlSize::Compact)
    .accessible("Model")
    .activate(move || open.set(!open.peek()))
    .into()
}

/// Favourite and recent models, effort levels, All models and Manage accounts, above the chip.
pub(super) fn panel(app: &App, open: Signal<bool>) -> Option<NodeBuilder> {
    if !open.get() {
        return None;
    }
    let snapshot = app.model.get().snapshot.clone();
    let current = ModelId::new(snapshot.endpoint.clone(), snapshot.model.clone());
    let pinned = home()
        .map(|home| summonr_agent::recent::pinned(&home))
        .unwrap_or_default();
    let mut menu = menu(open).label("Model").width(super::super::style::MENU);
    let friends = app.window.friends.get();
    for (endpoint, rows) in groups(&current, &pinned, &friends) {
        for row in rows {
            menu = menu.item(item(app, &endpoint, row));
        }
    }
    menu = menu.separator();
    for level in snapshot.levels.iter().copied() {
        let set = app.clone();
        let item =
            MenuItem::new(level.label()).activate(move || set.effort_command(level.as_str()));
        menu = menu.item(if snapshot.effort == Some(level) {
            item.hint(CURRENT)
        } else {
            item
        });
    }
    if !snapshot.levels.is_empty() {
        menu = menu.separator();
    }
    let (all, accounts) = (app.clone(), app.clone());
    Some(
        menu.item(MenuItem::new("All models…").activate(move || all.begin_model_edit()))
            .item(MenuItem::new("Manage accounts").activate(move || {
                super::super::settings::open(&accounts, super::super::settings::Page::Accounts)
            }))
            .into(),
    )
}

fn item(app: &App, endpoint: &str, row: ModelId) -> MenuItem {
    let pick = app.clone();
    let name = title(endpoint, &row.model);
    let Some(friend) = Friend::of(endpoint) else {
        return MenuItem::new(name)
            .hint(account(endpoint))
            .activate(move || pick.send_model(None, row.clone()));
    };
    MenuItem::new(name)
        .hint(friend.provider)
        .activate(move || super::super::friends::launch(&pick, friend, row.model.clone()))
}

fn title(endpoint: &str, model: &str) -> String {
    Friend::of(endpoint)
        .filter(|friend| friend.model == model)
        .map_or_else(|| model.to_owned(), |friend| friend.name.to_owned())
}

fn placeholder(row: &ModelId) -> bool {
    Friend::of(&row.endpoint).is_some_and(|friend| friend.model == row.model)
}

fn home() -> Option<std::path::PathBuf> {
    summonr_core::dirs::summonr_home().ok()
}

fn groups(current: &ModelId, pinned: &[ModelId], friends: &[Seen]) -> Vec<(String, Vec<ModelId>)> {
    let recent = home()
        .map(|home| summonr_agent::recent::load(&home))
        .unwrap_or_default();
    let mut seen: Vec<ModelId> = Vec::new();
    for row in std::iter::once(current.clone())
        .chain(pinned.iter().cloned())
        .chain(
            friends
                .iter()
                .filter(|seen| seen.state != State::Missing)
                .flat_map(|seen| {
                    let route = seen.friend.route;
                    seen.friend
                        .models()
                        .into_iter()
                        .map(|model| ModelId::new(route.to_owned(), model))
                }),
        )
        .chain(recent)
    {
        if !row.model.is_empty()
            && !placeholder(&row)
            && !seen.contains(&row)
            && seen.len() < ROWS_MAX
        {
            seen.push(row);
        }
    }
    let mut groups: Vec<(String, Vec<ModelId>)> = Vec::new();
    for row in seen {
        match groups
            .iter_mut()
            .find(|(endpoint, _)| *endpoint == row.endpoint)
        {
            Some((_, rows)) => rows.push(row),
            None => groups.push((row.endpoint.clone(), vec![row])),
        }
    }
    groups
}

fn account(endpoint: &str) -> String {
    summonr_agent::endpoint::label(endpoint).map_or_else(|| endpoint.to_string(), str::to_string)
}
