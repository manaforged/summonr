use sigil::prelude::Paint;
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, WidgetLayoutExt, container,
    text_field, virtual_list,
};

use super::prelude::*;
use super::{App, canvas};
use crate::endpoint::State;
use crate::tui::sigil_view::app::{Login, matching, section};
use crate::{AuthInfo, LoginFlow};

pub(in crate::tui::sigil_view) fn keys() -> Bindings {
    alt("d", |app, event| {
        event.stop_propagation();
        app.confirm_logout();
    })
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    let title = match app.auth_choice() {
        Some(choice) if app.picker.open.peek() => format!("Models › {}", choice.name),
        Some(_) | None => "Providers".to_string(),
    };
    shell(
        app.auth.open,
        title,
        app.viewport.peek().0,
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .children_reactive(move || {
                let name = paint
                    .auth_choice()
                    .map(|choice| choice.name)
                    .unwrap_or_default();
                match paint.auth.step.get() {
                    Login::List(note) => choices(&paint, &note),
                    Login::Key(note) => entering(&paint, &note),
                    Login::Checking => {
                        vec![line("Checking the key…"), hint("esc cancel", String::new())]
                    }
                    Login::SignIn {
                        text,
                        url,
                        code,
                        asking,
                        note,
                    } => signing(&paint, &name, [text, url, code, note], asking),
                    Login::Confirm => vec![
                        line(format!("Sign out of {name}?")),
                        hint("↵ yes · esc no", String::new()),
                    ],
                }
            }),
    )
}

fn line(text: impl Into<String>) -> NodeBuilder {
    container().child(
        sigil::kernel::text(text.into())
            .nowrap()
            .text_color(sigil::prelude::Token::OnSurface)
            .font_size(14.0),
    )
}

fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match parts.last_mut() {
            Some(last)
                if sigil::app::cell_width(last) + 1 + sigil::app::cell_width(word) <= width =>
            {
                last.push(' ');
                last.push_str(word);
            }
            Some(_) | None => parts.push(crate::text::ellipsize(word, width)),
        }
    }
    parts
}

fn span(app: &App) -> usize {
    let (cols, _) = app.viewport.get();
    (canvas::dialog_width(cols, canvas::DIALOG_CELLS) / canvas::cell().w) as usize
}

fn chip(choice: &AuthInfo, state: Option<&State>) -> String {
    state
        .filter(|state| state.failed())
        .map_or_else(|| choice.state(), ToString::to_string)
}

fn action(choice: &AuthInfo, state: &str) -> String {
    match (choice.flow, choice.env) {
        (LoginFlow::Browser, _) => "↵ opens the browser to sign in".to_string(),
        (LoginFlow::Device, _) => "↵ shows a device code to sign in".to_string(),
        (LoginFlow::Key, Some(env)) => format!("key comes from ${env}"),
        (LoginFlow::Key, None) if choice.stored() => "↵ replaces the key".to_string(),
        (LoginFlow::Key, None) if state == State::NoKey.word() => {
            "no key needed · ↵ sets a key if the server asks for one".to_string()
        }
        (LoginFlow::Key, None) => "↵ opens a field to paste the key".to_string(),
    }
}

const SEARCH: &str = "Search providers";

fn choices(app: &App, note: &str) -> Vec<NodeBuilder> {
    let paint = app.clone();
    let mut nodes = Vec::new();
    for part in wrapped(note, span(app).saturating_sub(10)) {
        nodes.push(band(part, note.starts_with('✗')));
    }
    nodes.push(field(app, SEARCH, false));
    nodes.push(
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .children_reactive(move || listing(&paint)),
    );
    nodes
}

fn listing(app: &App) -> Vec<NodeBuilder> {
    let source = app.clone();
    let rows = matching(app.auth.choices.get(), &app.auth.filter.get());
    let selected = app.auth.sel.get().min(rows.len().saturating_sub(1));
    let Some(choice) = rows.get(selected) else {
        return vec![group("No providers match."), hint("esc close", "0 / 0")];
    };
    let states = app.picker.groups.get();
    let mut lookup: Vec<(String, String, Option<usize>)> = Vec::new();
    let (mut shown, mut header) = (0, 0);
    for (index, row) in rows.iter().enumerate() {
        let tier = section(row);
        if index == 0 || tier != section(&rows[index - 1]) {
            let title = ["connected", "local", "not connected"][usize::from(tier.min(2))];
            lookup.push((title.to_string(), String::new(), None));
        }
        if index == selected {
            shown = lookup.len();
            header = lookup
                .iter()
                .rposition(|(_, _, index)| index.is_none())
                .unwrap_or(0);
        }
        lookup.push((
            row.name.clone(),
            chip(row, states.get(&row.name)),
            Some(index),
        ));
    }
    let visible = canvas::overlay_shown(lookup.len() + 3, app.viewport.get().1).saturating_sub(3);
    let anchor = if shown < visible {
        0
    } else if shown - header < visible {
        header
    } else {
        shown + 1 - visible
    };
    let state = chip(choice, states.get(&choice.name));
    let keys = if choice.stored() {
        "↑↓ move · ↵ open · alt+d sign out · esc close"
    } else {
        "↑↓ move · ↵ open · esc close"
    };
    vec![
        container().layout(LayoutStyle::block().fill_width()).child(
            virtual_list(
                lookup,
                move |_, (name, chip, index): &(String, String, Option<usize>)| match index {
                    Some(index) => entry(source.clone(), name, chip, *index, *index == selected),
                    None => group(name.clone()),
                },
            )
            .row_height(canvas::overlay_list_row())
            .height(canvas::overlay_list_row() * visible as f32)
            .viewport_height(canvas::overlay_list_row() * visible as f32)
            .scroll_into_view(anchor),
        ),
        group(crate::text::ellipsize(
            &format!("{} — {}", choice.name, action(choice, &state)),
            span(app).saturating_sub(10),
        )),
        hint(keys, format!("{} / {}", selected + 1, rows.len())),
    ]
}

fn entry(app: App, name: &str, chip: &str, index: usize, active: bool) -> NodeBuilder {
    let choose = app.clone();
    pick(active, name.to_string(), chip.to_string()).pointer_down(move |event| {
        choose.auth.sel.set(index);
        choose.commit_auth();
        event.stop_propagation();
    })
}

fn field(app: &App, placeholder: &'static str, secret: bool) -> NodeBuilder {
    let typing = app.clone();
    let save = app.clone();
    let filter = placeholder == SEARCH;
    let value = if filter {
        app.auth.filter
    } else {
        app.auth.key
    };
    let field = text_field(value)
        .placeholder(placeholder)
        .focus_when(app.auth.focus)
        .change(move |text| {
            if filter {
                typing.replace_auth_filter(text.to_string());
            } else {
                typing.replace_auth_key(text.to_string());
            }
        })
        .submit(move |_| save.commit_auth())
        .height(canvas::overlay_field());
    container().child(if secret { field.password() } else { field })
}

fn entering(app: &App, note: &str) -> Vec<NodeBuilder> {
    let count = app.clone();
    let choice = app.auth_choice();
    let name = choice.as_ref().map(|choice| choice.name.clone());
    let width = span(app).saturating_sub(10);
    let mut nodes = vec![
        line(format!("Paste the {} API key.", name.unwrap_or_default())),
        field(app, "paste or type the api key", true),
    ];
    nodes.extend(
        wrapped(note, width)
            .into_iter()
            .map(|part| band(part, true)),
    );
    nodes.push(container().children_reactive(move || {
        let characters = count.auth.key.get().chars().count();
        vec![hint(
            "↵ save and check · esc back",
            crate::text::plural(characters, "char"),
        )]
    }));
    nodes
}

fn signing(app: &App, name: &str, lines: [String; 4], asking: bool) -> Vec<NodeBuilder> {
    let [text, url, code, note] = lines;
    let wide = span(app).saturating_sub(10);
    let width = wide.saturating_sub(16);
    let mut nodes = vec![group(format!("Sign in to {name}"))];
    if !url.is_empty() {
        nodes.push(line(format!(
            "1. Open         {}",
            crate::text::ellipsize(&url, width)
        )));
    }
    if !code.is_empty() {
        nodes.push(line(format!("2. Enter code   {code}")));
    }
    nodes.extend(wrapped(&text, wide).into_iter().map(line));
    if asking {
        nodes.push(field(app, "Paste the code if the browser shows one", false));
    }
    if !note.is_empty() {
        nodes.extend(
            wrapped(&note, wide)
                .into_iter()
                .map(|part| band(part, true)),
        );
        nodes.push(hint("↵ try again · esc back", String::new()));
        return nodes;
    }
    let clock = app.clone();
    let owner = name.to_string();
    nodes.push(container().children_reactive(move || {
        let spin = sigil::prelude::spinner_frame(clock.ticks.get());
        let seconds = clock
            .model
            .peek()
            .busy_since
            .map_or(0, |(_, since)| since.elapsed().as_secs());
        vec![line(format!(
            "{spin} Waiting for {owner}…  {}:{:02}",
            seconds / 60,
            seconds % 60
        ))]
    }));
    let keys = if asking {
        "↵ send code · esc cancel"
    } else {
        "esc cancel"
    };
    nodes.push(hint(keys, String::new()));
    nodes
}
