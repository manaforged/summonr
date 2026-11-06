use std::collections::BTreeMap;

use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, LayoutStyle, NodeBuilder, Signal, Tone, Widget, button, chip, container,
    empty_state, icon, signal, text_field,
};

use super::super::App;
use super::super::style::{ICON, S, XL};
use super::parts::{row, section};
use summonr_agent::endpoint::{AuthInfo, State};

const SEARCH_FROM: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Group {
    Connected,
    Local,
    Available,
}

impl Group {
    const ALL: [Self; 3] = [Self::Connected, Self::Local, Self::Available];

    const fn title(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Local => "On this computer",
            Self::Available => "Available",
        }
    }

    fn of(choice: &AuthInfo, state: Option<State>) -> Self {
        if choice.keyless() {
            Self::Local
        } else if choice.configured || state == Some(State::Ready) {
            Self::Connected
        } else {
            Self::Available
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Setup,
    SignOut,
    Models,
}

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    load(app);
    let query = signal(String::new());
    let list = app.clone();
    let mut nodes = Vec::new();
    if app.auth.choices.peek().len() >= SEARCH_FROM {
        nodes.push(search(query));
    }
    nodes.push(
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .align_items(AlignItems::Stretch)
                    .gap(XL),
            )
            .children_reactive(move || groups(&list, &query.get())),
    );
    nodes
}

fn load(app: &App) {
    if app.auth.open.peek() {
        return;
    }
    app.auth
        .choices
        .set(crate::app::auth::providers(&app.picker.groups.peek()));
    if !app.picker.open.peek() {
        app.list_models(None);
    }
}

fn search(query: Signal<String>) -> NodeBuilder {
    text_field(query)
        .placeholder("Search accounts")
        .leading(icon(Icon::Search).px(ICON))
        .clearable()
        .into()
}

fn groups(app: &App, query: &str) -> Vec<NodeBuilder> {
    let choices = app.auth.choices.get();
    let states = app.picker.groups.get();
    if choices.is_empty() {
        return vec![empty(app)];
    }
    let query = query.to_lowercase();
    let shown: Vec<&AuthInfo> = choices
        .iter()
        .filter(|choice| choice.matches(&query))
        .collect();
    if shown.is_empty() {
        return vec![empty_state("No matches", "")];
    }
    Group::ALL
        .into_iter()
        .filter_map(|group| {
            let rows: Vec<NodeBuilder> = shown
                .iter()
                .filter(|choice| Group::of(choice, states.get(&choice.name).copied()) == group)
                .map(|choice| entry(app, choice, &states))
                .collect();
            (!rows.is_empty()).then(|| section(Some(group.title()), rows))
        })
        .collect()
}

fn empty(app: &App) -> NodeBuilder {
    let open = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(S)
                .align_items(AlignItems::Center),
        )
        .child(empty_state("No accounts yet", ""))
        .child(
            button("Add account")
                .primary()
                .activate(move || open.open_providers()),
        )
}

fn entry(app: &App, choice: &AuthInfo, states: &BTreeMap<String, State>) -> NodeBuilder {
    let state = states.get(&choice.name).copied();
    let (label, tone) = status(choice, state);
    let controls = container()
        .layout(
            LayoutStyle::flex_row()
                .gap(S)
                .align_items(AlignItems::Center),
        )
        .child(chip(label, tone).dot());
    let controls = match action(choice, state) {
        Some((word, act)) => controls.child(control(app, choice, word, act)),
        None => controls,
    };
    row(&display(choice), about(choice).as_deref(), controls)
}

fn control(app: &App, choice: &AuthInfo, word: &'static str, act: Action) -> NodeBuilder {
    let app = app.clone();
    let name = choice.name.clone();
    button(word)
        .accessible(format!("{word}, {}", display(choice)))
        .activate(move || run(&app, &name, act))
        .into()
}

fn display(choice: &AuthInfo) -> String {
    if let Some(label) = summonr_agent::endpoint::label(&choice.name) {
        return label.to_owned();
    }
    let mut chars = choice.title().chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn run(app: &App, name: &str, act: Action) {
    if act == Action::Models {
        app.begin_model_edit();
        app.replace_model_draft(name.to_owned());
        return;
    }
    app.begin_auth(None);
    let Some(at) = app
        .filtered_auth()
        .iter()
        .position(|choice| choice.name == name)
    else {
        return;
    };
    app.auth.list.cursor.set(at);
    match act {
        Action::Setup => app.commit_auth(),
        Action::SignOut => app.confirm_logout(),
        Action::Models => {}
    }
}

fn status(choice: &AuthInfo, state: Option<State>) -> (&'static str, Tone) {
    match state {
        Some(State::KeyRejected) => return ("Key rejected", Tone::Error),
        Some(State::Offline) => return ("Offline", Tone::Error),
        Some(State::RateLimited) => return ("Rate limited", Tone::Warn),
        Some(State::Error) => return ("Not working", Tone::Error),
        Some(
            State::Ready
            | State::SignIn
            | State::NoKey
            | State::NeedsKey
            | State::NeedsModel
            | State::Checking
            | State::Loading,
        )
        | None => {}
    }
    if choice.unmodeled() {
        return ("Needs a model", Tone::Warn);
    }
    if choice.keyless() {
        return ("No key needed", Tone::Info);
    }
    match (choice.configured, choice.flow.signs_in(), choice.env) {
        (true, true, _) => ("Signed in", Tone::Success),
        (true, false, Some(_)) => ("Key from system", Tone::Success),
        (true, false, None) => ("Key saved", Tone::Success),
        (false, ..) => ("Not set up", Tone::Neutral),
    }
}

fn action(choice: &AuthInfo, state: Option<State>) -> Option<(&'static str, Action)> {
    if choice.keyless() {
        return Some(("Models…", Action::Models));
    }
    let broken = state.is_some_and(State::failed);
    if choice.stored() && !broken {
        return Some(("Sign out", Action::SignOut));
    }
    if choice.configured && !broken {
        return None;
    }
    if choice.login.signs_in() {
        Some(("Sign in", Action::Setup))
    } else if choice.env.is_some() {
        None
    } else {
        Some(("Add key", Action::Setup))
    }
}

fn about(choice: &AuthInfo) -> Option<String> {
    if choice.keyless() {
        return Some("Runs on this computer, free to use".to_owned());
    }
    if let Some(env) = choice.env.filter(|_| choice.configured) {
        return Some(format!("Key comes from {env}"));
    }
    if choice.login.signs_in() && !choice.plan.is_empty() {
        return Some(format!("Use your {} plan", choice.plan));
    }
    if choice.key {
        return Some("Paste an API key from their website".to_owned());
    }
    None
}
