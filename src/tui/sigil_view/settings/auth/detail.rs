use sigil::prelude::{NodeBuilder, container};

use super::{App, line};
use crate::AuthInfo;
use crate::catalog::Cost;
use crate::endpoint::State;

const MODEL_ROWS: usize = 6;

pub(super) fn summary(choice: &AuthInfo, state: &str) -> String {
    format!("{} — {}", choice.name, primary(choice, state))
}

pub(super) fn pane(app: &App, choice: &AuthInfo, state: &str, rows: usize) -> Vec<NodeBuilder> {
    let mut nodes = vec![
        line(choice.name.clone()),
        line(format!("key       {}", key_line(choice, state))),
        line(format!("sign-in   {}", login_line(choice))),
    ];
    let models = models(app, &choice.name);
    if !models.is_empty() {
        nodes.push(line(String::new()));
        nodes.push(line("models"));
        let shown = models.len().min(MODEL_ROWS).min(rows.saturating_sub(7));
        nodes.extend(
            models
                .iter()
                .take(shown)
                .map(|row| line(format!("  {row}"))),
        );
        if models.len() > shown {
            nodes.push(line(format!("  … {} more", models.len() - shown)));
        }
    }
    nodes.push(line(String::new()));
    nodes.push(line(format!("↵         {}", primary(choice, state))));
    if choice.dual() && choice.env.is_none() {
        nodes.push(line("alt+k     paste an API key"));
    }
    if choice.stored() {
        nodes.push(line("alt+d     sign out"));
    }
    nodes.push(container());
    nodes
}

fn key_line(choice: &AuthInfo, state: &str) -> String {
    if !choice.key {
        return "not offered".to_string();
    }
    let signed_in = choice.flow.signs_in() && choice.configured;
    match (choice.env, choice.configured && !signed_in) {
        (Some(env), _) if state == State::KeyRejected.word() => format!("${env} · rejected"),
        (Some(env), _) => format!("${env}"),
        (None, true) if state == State::KeyRejected.word() => "stored · rejected".to_string(),
        (None, true) => choice
            .source
            .map_or_else(|| "stored".to_string(), |source| source.label().to_string()),
        (None, false) => "none".to_string(),
    }
}

fn login_line(choice: &AuthInfo) -> String {
    if !choice.login.signs_in() {
        return "not offered".to_string();
    }
    if choice.flow.signs_in() && choice.configured {
        return format!("signed in · {}", choice.plan);
    }
    format!("{} · not signed in", choice.plan)
}

fn primary(choice: &AuthInfo, state: &str) -> String {
    if choice.login.signs_in() {
        return format!("sign in with {}", choice.plan);
    }
    match choice.env {
        Some(env) => format!("key comes from ${env}"),
        None if choice.stored() => "replace the key".to_string(),
        None if state == State::NoKey.word() => "set a key if the server asks for one".to_string(),
        None => "paste an API key".to_string(),
    }
}

fn models(app: &App, name: &str) -> Vec<String> {
    let facts = app.picker.facts.get();
    app.picker
        .choices
        .get()
        .iter()
        .filter(|seat| seat.endpoint == name)
        .map(|seat| match facts.get(seat).map(|model| model.cost) {
            Some(Cost::Plan) => format!("{} · plan", seat.model),
            Some(Cost::Free) => format!("{} · free", seat.model),
            Some(Cost::Unknown | Cost::Metered) | None => seat.model.clone(),
        })
        .collect()
}
