use sigil::prelude::{NodeBuilder, Token};

use super::super::models::{price, tone};
use super::App;
use crate::AuthInfo;
use crate::endpoint::State;
use crate::tui::sigil_view::look::Look;
use crate::tui::sigil_view::ui::{blank, field as kv, heading, part as text};

const CONTEXT: usize = 8;
const PRICE: usize = 16;
const NAME: usize = 44;

pub(super) fn summary(choice: &AuthInfo, state: &str) -> String {
    format!("{}: {}", choice.name, primary(choice, state))
}

pub(super) fn pane(
    app: &App,
    choice: &AuthInfo,
    (state, tier): (&str, Option<State>),
    rows: usize,
    width: usize,
) -> Vec<NodeBuilder> {
    let color = match tier {
        Some(tier) => tone(tier),
        None if choice.configured || choice.keyless() => Token::Success11,
        None => Token::Gray10,
    };
    let mut nodes = vec![
        heading(vec![
            (choice.name.clone(), Look::sm(Token::OnSurface).bold()),
            text("   ", Token::Gray10),
            text(state, color),
        ]),
        blank(),
    ];
    let key = key_line(choice, state);
    if let Some(key) = key {
        let rejected = tier == Some(State::KeyRejected);
        nodes.push(kv(
            "key",
            key,
            if rejected {
                Token::Error11
            } else {
                Token::OnSurface
            },
        ));
    }
    if choice.login.signs_in() {
        nodes.push(kv("sign-in", login_line(choice), Token::OnSurface));
    }
    let models = models(app, &choice.name);
    if !models.is_empty() {
        let name = width.saturating_sub(CONTEXT + PRICE + 2).min(NAME);
        nodes.push(blank());
        nodes.push(heading(vec![
            text(
                format!("{:<name$}", format!("models  {}", models.len())),
                Token::Gray10,
            ),
            text(format!("{:>CONTEXT$}  ", "CONTEXT"), Token::Gray9),
            text("$/1M IN/OUT", Token::Gray9),
        ]));
        let room = rows.saturating_sub(nodes.len() + 5);
        let shown = models.len().min(room);
        nodes.extend(models.iter().take(shown).map(|(id, context, cost)| {
            heading(vec![
                text(
                    format!(
                        "  {:<name$}",
                        crate::text::ellipsize(id, name.saturating_sub(2))
                    ),
                    Token::Gray12,
                ),
                text(format!("{context:>CONTEXT$}  "), Token::Gray11),
                text(cost.clone(), Token::Gray11),
            ])
        }));
        if models.len() > shown {
            nodes.push(heading(vec![text(
                format!("  ▸ {} more", models.len() - shown),
                Token::Accent11,
            )]));
        }
    }
    nodes.push(blank());
    nodes.push(heading(vec![
        text("↵ ", Token::Accent11),
        text(primary(choice, state), Token::OnSurface),
    ]));
    if choice.paste_key() {
        nodes.push(heading(vec![
            text("alt+k ", Token::Accent11),
            text("paste an API key", Token::OnSurface),
        ]));
    }
    if choice.stored() {
        nodes.push(heading(vec![
            text("alt+d ", Token::Accent11),
            text("sign out", Token::OnSurface),
        ]));
    }
    nodes
}

fn key_line(choice: &AuthInfo, state: &str) -> Option<String> {
    if choice.keyless() {
        return Some("none needed".to_string());
    }
    if !choice.key {
        return None;
    }
    let signed_in = choice.flow.signs_in() && choice.configured;
    Some(match (choice.env, choice.configured && !signed_in) {
        (Some(env), _) if state == State::KeyRejected.word() => format!("${env} · rejected"),
        (Some(env), _) => format!("${env}"),
        (None, true) if state == State::KeyRejected.word() => "stored · rejected".to_string(),
        (None, true) => choice
            .source
            .map_or_else(|| "stored".to_string(), |source| source.label().to_string()),
        (None, false) => "none".to_string(),
    })
}

fn login_line(choice: &AuthInfo) -> String {
    if choice.flow.signs_in() && choice.configured {
        return format!("signed in · {}", choice.plan);
    }
    format!("{} · not signed in", choice.plan)
}

fn primary(choice: &AuthInfo, state: &str) -> String {
    if choice.login.signs_in() {
        return format!("sign in with {}", choice.plan);
    }
    if choice.keyless() {
        return "no key needed · set one if the server asks".to_string();
    }
    match choice.env {
        Some(env) => format!("key comes from ${env}"),
        None if choice.stored() => "replace the key".to_string(),
        None if state == State::NoKey.word() => "set a key if the server asks for one".to_string(),
        None => "set a key".to_string(),
    }
}

fn models(app: &App, name: &str) -> Vec<(String, String, String)> {
    let facts = app.picker.facts.get();
    app.picker
        .choices
        .get()
        .iter()
        .filter(|seat| seat.endpoint == name)
        .map(|seat| {
            let fact = facts.get(seat);
            let context = fact.map(crate::catalog::Model::window).unwrap_or_default();
            (seat.model.clone(), context, price(fact))
        })
        .collect()
}
