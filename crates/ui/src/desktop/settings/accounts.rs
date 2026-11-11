use std::collections::BTreeMap;

use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, NodeBuilder, Signal, Tone, Widget, button, chip, empty_state, icon, signal,
    text_field,
};

use super::super::App;
use super::super::friends::{self, Seen, State};
use super::super::style::{ICON, S, XL};
use super::parts::{empty_accounts, row, section};
use summonr_agent::endpoint::{AuthInfo, Friend, State as Health};

const SEARCH_FROM: usize = 8;
const AGENTS: &str = "Agents";

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

    fn of(choice: &AuthInfo, state: Option<Health>) -> Self {
        if choice.keyless() {
            Self::Local
        } else if choice.configured || state == Some(Health::Ready) {
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
    friends::refresh(app);
    let query = signal(String::new());
    let list = app.clone();
    let mut nodes = vec![agents(app)];
    if app.auth.choices.peek().len() >= SEARCH_FROM {
        nodes.push(search(query));
    }
    nodes.push(
        sigil::prelude::column()
            .fill_width()
            .align_items(AlignItems::Stretch)
            .gap(XL)
            .children_reactive(move || groups(&list, &query.get())),
    );
    nodes
}

fn agents(app: &App) -> NodeBuilder {
    let app = app.clone();
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .children_reactive(move || {
            let rows: Vec<NodeBuilder> = app
                .window
                .friends
                .get()
                .iter()
                .map(|seen| agent(&app, *seen))
                .collect();
            if rows.is_empty() {
                return Vec::new();
            }
            vec![section(Some(AGENTS), rows)]
        })
}

fn agent(app: &App, seen: Seen) -> NodeBuilder {
    let friend = seen.friend;
    let (label, tone) = match seen.state {
        State::SignedIn => ("Signed in", Tone::Success),
        State::SignedOut => ("Not signed in", Tone::Neutral),
        State::Missing => ("Not installed", Tone::Neutral),
    };
    let controls = sigil::prelude::row()
        .gap(S)
        .align_items(AlignItems::Center)
        .child(chip(label, tone).dot());
    let (act, word): (fn(&App, Friend), &str) = match seen.state {
        State::SignedIn => {
            return row(friend.name, Some(&about_agent(friend)), controls);
        }
        State::SignedOut => (friends::sign_in, "Sign in"),
        State::Missing => (friends::install, "Install"),
    };
    let app = app.clone();
    let controls = controls.child(
        button(word)
            .accessible(format!("{word}, {}", friend.name))
            .activate(move || act(&app, friend)),
    );
    row(friend.name, Some(&about_agent(friend)), controls)
}

fn about_agent(friend: Friend) -> String {
    format!("Runs chats through your own {}", friend.name)
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
        return vec![empty_accounts(app, "No accounts yet")];
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

fn entry(app: &App, choice: &AuthInfo, states: &BTreeMap<String, Health>) -> NodeBuilder {
    let state = states.get(&choice.name).copied();
    let (label, tone) = status(choice, state);
    let controls = sigil::prelude::row()
        .gap(S)
        .align_items(AlignItems::Center)
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
    if act == Action::Setup
        && let Some(friend) = Friend::of(name)
    {
        friends::sign_in(app, friend);
        return;
    }
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

fn status(choice: &AuthInfo, state: Option<Health>) -> (&'static str, Tone) {
    match state {
        Some(Health::KeyRejected) => return ("Key rejected", Tone::Error),
        Some(Health::Offline) => return ("Offline", Tone::Error),
        Some(Health::RateLimited) => return ("Rate limited", Tone::Warning),
        Some(Health::Error) => return ("Not working", Tone::Error),
        Some(
            Health::Ready
            | Health::SignIn
            | Health::NoKey
            | Health::NeedsKey
            | Health::NeedsModel
            | Health::Checking
            | Health::Loading,
        )
        | None => {}
    }
    if choice.unmodeled() {
        return ("Needs a model", Tone::Warning);
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

fn action(choice: &AuthInfo, state: Option<Health>) -> Option<(&'static str, Action)> {
    if choice.keyless() {
        return Some(("Models…", Action::Models));
    }
    let broken = state.is_some_and(Health::failed);
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
