use std::rc::Rc;

use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, LayoutStyle, NodeBuilder, Signal, Tone, Widget, button, chip, container,
    empty_state, icon, signal, text_field,
};
use sigil::reactive::untrack;

use super::super::super::App;
use super::super::super::style::{ICON, S, XL};
use super::super::parts::{account, empty_accounts, row, section};
use summonr_agent::catalog::{Cost, Model};
use summonr_agent::recent::ModelId;

const PER_ACCOUNT: usize = 5;
const MATCHES: usize = 40;

struct Account {
    name: String,
    models: Vec<Model>,
}

/// The catalog's models for each signed-in account, with search and a Use button per model.
pub(super) fn view(app: &App) -> NodeBuilder {
    let query = signal(String::new());
    let snapshot = app.model.peek().snapshot.clone();
    let using = signal(ModelId::new(snapshot.endpoint, snapshot.model));
    let paint = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .align_items(AlignItems::Stretch)
                .gap(S),
        )
        .child(search(query))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .align_items(AlignItems::Stretch)
                        .gap(XL),
                )
                .children_reactive(move || {
                    paint.picker.groups.with(|_| ());
                    let accounts = Rc::new(untrack(|| accounts(&using.peek().endpoint)));
                    let app = paint.clone();
                    vec![
                        container()
                            .layout(
                                LayoutStyle::flex_column()
                                    .fill_width()
                                    .align_items(AlignItems::Stretch)
                                    .gap(XL),
                            )
                            .children_reactive(move || body(&app, &accounts, &query.get(), using)),
                    ]
                }),
        )
}

fn search(query: Signal<String>) -> NodeBuilder {
    text_field(query)
        .placeholder("Search models")
        .leading(icon(Icon::Search).px(ICON))
        .clearable()
        .into()
}

fn accounts(current: &str) -> Vec<Account> {
    let cached = summonr_agent::catalog::cached();
    let mut accounts: Vec<Account> = cached
        .iter()
        .filter(|(name, _)| {
            summonr_agent::endpoint::auth_info(name)
                .is_ok_and(|info| info.configured || info.keyless())
        })
        .map(|(name, listing)| Account {
            name: name.clone(),
            models: usable(name, &listing.models),
        })
        .filter(|account| !account.models.is_empty())
        .collect();
    accounts.sort_by_key(|entry| (entry.name != current, account(&entry.name)));
    accounts
}

fn usable(name: &str, models: &[Model]) -> Vec<Model> {
    let pairs: Vec<(&str, &Model)> = models.iter().map(|model| (name, model)).collect();
    let fresh = summonr_agent::catalog::current(&pairs);
    models
        .iter()
        .zip(fresh)
        .filter(|(model, fresh)| *fresh && model.usable())
        .map(|(model, _)| model.clone())
        .collect()
}

fn body(app: &App, accounts: &[Account], query: &str, using: Signal<ModelId>) -> Vec<NodeBuilder> {
    if accounts.is_empty() {
        return vec![empty_accounts(app, "No models yet")];
    }
    let query = query.trim().to_lowercase();
    let now = using.get();
    let mut room = MATCHES;
    let mut cut = false;
    let mut nodes = Vec::new();
    for entry_acc in accounts {
        let found: Vec<&Model> = entry_acc
            .models
            .iter()
            .filter(|model| matches(model, &query))
            .collect();
        let take = if query.is_empty() { PER_ACCOUNT } else { room };
        cut |= found.len() > take;
        let rows: Vec<NodeBuilder> = found
            .into_iter()
            .take(take)
            .map(|model| entry(app, &entry_acc.name, model, (&now, using)))
            .collect();
        room = room.saturating_sub(rows.len());
        if !rows.is_empty() {
            nodes.push(section(Some(&account(&entry_acc.name)), rows));
        }
    }
    if nodes.is_empty() {
        return vec![empty_state("No matches", "")];
    }
    if cut {
        nodes.push(more(app));
    }
    nodes
}

fn matches(model: &Model, query: &str) -> bool {
    query.is_empty()
        || model.id.to_lowercase().contains(query)
        || model.label().to_lowercase().contains(query)
}

fn entry(
    app: &App,
    endpoint: &str,
    model: &Model,
    (now, using): (&ModelId, Signal<ModelId>),
) -> NodeBuilder {
    let id = ModelId::new(endpoint, model.id.clone());
    let label = model.label().to_owned();
    let control: NodeBuilder = if *now == id {
        chip("In use", Tone::Success).dot().into_node_builder()
    } else {
        let pick = app.clone();
        button("Use")
            .accessible(format!("Use {label}"))
            .activate(move || {
                pick.send_model(None, id.clone());
                using.set(id.clone());
            })
            .into()
    };
    row(&label, details(model).as_deref(), control)
}

fn details(model: &Model) -> Option<String> {
    let window = model.window();
    let parts = [
        (!window.is_empty()).then(|| format!("{window} context")),
        cost(model),
        model.reasoning.then(|| "Thinks".to_owned()),
        model.vision.then(|| "Sees images".to_owned()),
    ];
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn cost(model: &Model) -> Option<String> {
    let shown = crate::settings::models::price(Some(model));
    match (model.cost, model.price.is_some()) {
        (Cost::Unknown, _) => None,
        (Cost::Metered, true) => Some(format!("{shown} per 1M")),
        (Cost::Free | Cost::Plan | Cost::Metered, _) => Some(shown),
    }
}

fn more(app: &App) -> NodeBuilder {
    let open = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .align_items(AlignItems::Center),
        )
        .child(
            button("All models…")
                .ghost()
                .activate(move || open.begin_model_edit()),
        )
}
