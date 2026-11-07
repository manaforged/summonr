use std::cell::Cell;

use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, ControlSize, JustifyContent, LayoutStyle, NodeBuilder, Signal, Tone, Widget,
    button, chip, container, empty_state, field, icon, notice_band, signal, text_field,
};
use sigil::reactive::defer;

use super::super::App;
use super::super::style::{ICON, L, M, S, Text, XS, text};
use super::mcp::{actions, item};
use super::parts::{section, select};
use summonr_agent::workflow::{self, ModelSelection, Role, Workflow};

const ROLES: [(Role, &str); 5] = [
    (Role::Scout, "Scout"),
    (Role::Implement, "Implement"),
    (Role::Judge, "Judge"),
    (Role::Summary, "Summary"),
    (Role::Title, "Title"),
];
const PRIMARY: &str = "Primary model";
const ENDPOINT_MODEL: &str = "Default model";
const MODEL: &str = "Model";
const DEFAULT: &str = "Default";
const NEW: &str = "New workflow";
const REMOVE: &str = "Remove";
const TAKEN: &str = "A workflow has that name";

#[derive(Clone, Copy)]
struct Page {
    adding: Signal<bool>,
    editing: Signal<Option<String>>,
    armed: Signal<Option<String>>,
    error: Signal<Option<String>>,
    name: Signal<String>,
    saved: Signal<u32>,
}

impl Page {
    fn wrote(self, outcome: summonr_agent::Result<()>) {
        match outcome {
            Ok(()) => self.error.set(None),
            Err(error) => self.error.set(Some(first_line(&error.to_string()))),
        }
        self.saved.update(|count| *count += 1);
    }
}

thread_local! {
    static ADDING: Cell<Option<Signal<bool>>> = const { Cell::new(None) };
}

fn adding() -> Signal<bool> {
    ADDING.with(|slot| {
        let adding = slot
            .get()
            .filter(|adding| adding.try_peek().is_some())
            .unwrap_or_else(|| signal(false));
        slot.set(Some(adding));
        adding
    })
}

/// Opens Settings → Workflows with the form for a new workflow.
pub(in crate::desktop) fn create(app: &App) {
    adding().set(true);
    super::open(app, super::Page::Workflows);
}

pub(super) fn view(_app: &App) -> Vec<NodeBuilder> {
    let page = Page {
        adding: adding(),
        editing: signal(None),
        armed: signal(None),
        error: signal(None),
        name: signal(String::new()),
        saved: signal(0),
    };
    vec![
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(M)
                    .align_items(AlignItems::Stretch),
            )
            .children_reactive(move || {
                page.saved.with(|_| ());
                let notice = page
                    .error
                    .get()
                    .filter(|_| !page.adding.get())
                    .map(|error| notice_band(error, Tone::Error));
                notice.into_iter().chain(body(page)).collect::<Vec<_>>()
            }),
    ]
}

fn body(page: Page) -> Vec<NodeBuilder> {
    let names = workflow::names().unwrap_or_default();
    let adding = page.adding.get();
    if names.is_empty() && !adding {
        return vec![section(
            None,
            vec![empty_state("No workflows yet", "").child(new_button(page).primary())],
        )];
    }
    let default = workflow::default_name().ok().flatten();
    let editing = page.editing.get();
    let mut rows: Vec<NodeBuilder> = names
        .iter()
        .map(|name| listed(page, name, default.as_deref() == Some(name.as_str())))
        .collect();
    rows.push(if adding {
        form(page, &names)
    } else {
        add_row(page)
    });
    let mut nodes = vec![section(None, rows)];
    if let Some(open) = editing.filter(|open| names.contains(open)) {
        nodes.push(editor(page, &open));
    }
    nodes
}

fn listed(page: Page, name: &str, default: bool) -> NodeBuilder {
    let summary = workflow::load(name).map_or_else(
        |error| first_line(&error.to_string()),
        |workflow| workflow.summary(),
    );
    let words = container()
        .layout(LayoutStyle::flex_column().gap(XS))
        .child(text(Text::Body, name.to_owned()))
        .child(text(Text::Meta, summary));
    let editing = page.editing.peek().as_deref() == Some(name);
    if page.armed.peek().as_deref() == Some(name) {
        return item(
            icon(Icon::Layers).px(ICON).into(),
            words,
            confirm(page, name),
        );
    }
    let mut trailing = actions();
    if default {
        trailing = trailing.child(chip(DEFAULT, Tone::Neutral));
    } else {
        let pick = name.to_owned();
        trailing = trailing.child(
            button("Make default")
                .ghost()
                .size(ControlSize::Compact)
                .accessible(format!("Make {name} the default"))
                .activate(move || {
                    let pick = pick.clone();
                    defer(move || page.wrote(workflow::set_default(&pick)));
                }),
        );
    }
    let open = name.to_owned();
    trailing = trailing.child(
        button(if editing { "Done" } else { "Edit" })
            .ghost()
            .size(ControlSize::Compact)
            .accessible(format!("Edit {name}"))
            .activate(move || {
                let next = (!editing).then(|| open.clone());
                defer(move || page.editing.set(next));
            }),
    );
    let arm = name.to_owned();
    trailing = trailing.child(
        button(REMOVE)
            .ghost()
            .tone(Tone::Error)
            .size(ControlSize::Compact)
            .accessible(format!("Remove {name}"))
            .activate(move || {
                let arm = arm.clone();
                defer(move || page.armed.set(Some(arm)));
            }),
    );
    item(icon(Icon::Layers).px(ICON).into(), words, trailing)
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
            button(REMOVE)
                .tone(Tone::Error)
                .size(ControlSize::Compact)
                .accessible(format!("Remove {name} now"))
                .activate(move || {
                    let name = name.clone();
                    defer(move || {
                        page.armed.set(None);
                        if page.editing.peek().as_deref() == Some(name.as_str()) {
                            page.editing.set(None);
                        }
                        page.wrote(workflow::remove(&name).map(drop));
                    });
                }),
        )
}

fn editor(page: Page, name: &str) -> NodeBuilder {
    let saved = match workflow::load(name) {
        Ok(saved) => saved,
        Err(error) => return notice_band(first_line(&error.to_string()), Tone::Error),
    };
    let accounts = accounts();
    let mut rows = Vec::new();
    for (role, label) in ROLES {
        let chosen = saved.selection(role).cloned();
        rows.push(endpoint_row(page, &saved, role, label, &accounts));
        if let Some(chosen) = chosen {
            rows.push(model_row(page, &saved, role, &chosen));
        }
    }
    section(Some(name), rows)
}

fn endpoint_row(
    page: Page,
    saved: &Workflow,
    role: Role,
    label: &str,
    accounts: &[String],
) -> NodeBuilder {
    let mut choices = accounts.to_vec();
    let chosen = saved.selection(role).map(|chosen| chosen.endpoint.clone());
    if let Some(chosen) = chosen.as_ref().filter(|chosen| !choices.contains(chosen)) {
        choices.push(chosen.clone());
    }
    let at = chosen
        .and_then(|chosen| choices.iter().position(|name| *name == chosen))
        .map_or(0, |at| at + 1);
    let options = std::iter::once(PRIMARY.to_owned())
        .chain(choices.iter().map(|name| account(name)))
        .collect();
    let name = saved.name.clone();
    select(label, None, options, at, move |index| {
        let selection = index
            .checked_sub(1)
            .and_then(|at| choices.get(at))
            .map(|endpoint| ModelSelection {
                endpoint: endpoint.clone(),
                model: None,
            });
        page.wrote(assign(&name, role, selection));
    })
}

fn model_row(page: Page, saved: &Workflow, role: Role, chosen: &ModelSelection) -> NodeBuilder {
    let mut models = models(&chosen.endpoint);
    if let Some(model) = chosen
        .model
        .as_ref()
        .filter(|model| !models.contains(model))
    {
        models.push(model.clone());
    }
    let at = chosen
        .model
        .as_ref()
        .and_then(|model| models.iter().position(|seen| seen == model))
        .map_or(0, |at| at + 1);
    let options = std::iter::once(ENDPOINT_MODEL.to_owned())
        .chain(models.iter().cloned())
        .collect();
    let (name, endpoint) = (saved.name.clone(), chosen.endpoint.clone());
    select(MODEL, None, options, at, move |index| {
        let selection = ModelSelection {
            endpoint: endpoint.clone(),
            model: index.checked_sub(1).and_then(|at| models.get(at)).cloned(),
        };
        page.wrote(assign(&name, role, Some(selection)));
    })
}

fn assign(name: &str, role: Role, selection: Option<ModelSelection>) -> summonr_agent::Result<()> {
    let mut saved = workflow::load(name)?;
    saved.assign(role, selection);
    workflow::save(&saved)
}

fn accounts() -> Vec<String> {
    summonr_agent::catalog::cached()
        .keys()
        .filter(|name| {
            summonr_agent::endpoint::auth_info(name)
                .is_ok_and(|info| info.configured || info.keyless())
        })
        .cloned()
        .collect()
}

fn models(endpoint: &str) -> Vec<String> {
    summonr_agent::catalog::listing(endpoint)
        .map(|listing| {
            listing
                .models
                .into_iter()
                .filter(summonr_agent::catalog::Model::usable)
                .map(|model| model.id)
                .collect()
        })
        .unwrap_or_default()
}

fn account(endpoint: &str) -> String {
    summonr_agent::endpoint::label(endpoint).map_or_else(|| endpoint.to_owned(), str::to_owned)
}

fn add_row(page: Page) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_x(L)
                .padding_y(M),
        )
        .child(new_button(page).ghost())
}

fn new_button(page: Page) -> sigil::widgets::ButtonBuilder {
    button(NEW).icon(Icon::Plus).activate(move || {
        page.error.set(None);
        defer(move || page.adding.set(true));
    })
}

fn form(page: Page, names: &[String]) -> NodeBuilder {
    let taken = names.to_vec();
    let submit = taken.clone();
    let ready = move || !page.name.get().trim().is_empty();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .align_items(AlignItems::Stretch)
                .gap(M)
                .padding_x(L)
                .padding_y(M),
        )
        .child(field(
            "Name",
            text_field(page.name)
                .placeholder("cheap-scout")
                .submit(move |_| save_new(page, &submit)),
        ))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .align_items(AlignItems::Stretch),
                )
                .children_reactive(move || {
                    page.error
                        .get()
                        .map(|error| notice_band(error, Tone::Error))
                }),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .gap(S)
                        .justify_content(JustifyContent::End),
                )
                .child(button("Cancel").ghost().activate(move || close(page)))
                .child(
                    button(NEW)
                        .primary()
                        .disabled(move || !ready())
                        .activate(move || save_new(page, &taken)),
                ),
        )
}

fn save_new(page: Page, taken: &[String]) {
    let fresh = Workflow::new(&page.name.peek());
    if taken.contains(&fresh.name) {
        return page.error.set(Some(TAKEN.to_owned()));
    }
    match workflow::save(&fresh) {
        Ok(()) => {
            page.editing.set(Some(fresh.name));
            close(page);
            page.saved.update(|count| *count += 1);
        }
        Err(error) => page.error.set(Some(first_line(&error.to_string()))),
    }
}

fn close(page: Page) {
    page.name.set(String::new());
    page.error.set(None);
    defer(move || page.adding.set(false));
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or(text).to_owned()
}
