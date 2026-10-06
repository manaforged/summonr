use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, Paint, Tone, WidgetLayoutExt, container, text_field,
};

use super::super::{App, canvas};
use crate::tui::sigil_view::app::Login;
use crate::tui::sigil_view::ui::{group, notice};

pub(super) const SEARCH: &str = "Search providers";

pub(super) fn form(app: &App) -> Vec<NodeBuilder> {
    let name = app
        .auth_choice()
        .map(|choice| choice.name)
        .unwrap_or_default();
    let form = match app.auth.step.get() {
        Login::List(_) => return Vec::new(),
        Login::Key(note) => entering(app, &note),
        Login::Checking => vec![line("Checking…")],
        Login::SignIn {
            text,
            url,
            code,
            asking,
            note,
        } => signing(app, &name, [text, url, code, note], asking),
        Login::Confirm => vec![line(format!("Sign out of {name}?"))],
    };
    vec![
        form.into_iter().fold(
            container().layout(
                LayoutStyle::flex_column()
                    .width(form_cells(app) as f32 * canvas::cell().w)
                    .gap(canvas::overlay_gap()),
            ),
            |node, child| node.child(child),
        ),
    ]
}

pub(super) fn line(text: impl Into<String>) -> NodeBuilder {
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

fn form_cells(app: &App) -> usize {
    let (cols, _) = app.viewport.get();
    ((canvas::dialog_width(cols, canvas::DIALOG_CELLS) / canvas::cell().w) as usize)
        .saturating_sub(2)
}

pub(super) fn notes(note: &str, width: usize) -> Vec<NodeBuilder> {
    let tone = if note.starts_with('✗') {
        Tone::Error
    } else {
        Tone::Neutral
    };
    wrapped(note, width)
        .into_iter()
        .map(|part| notice(part, tone))
        .collect()
}

pub(super) fn field(
    app: &App,
    placeholder: &'static str,
    secret: bool,
    focus: bool,
) -> NodeBuilder {
    let typing = app.clone();
    let save = app.clone();
    let filter = placeholder == SEARCH;
    let value = if filter {
        app.auth.filter
    } else {
        app.auth.key
    };
    let field = text_field(value).placeholder(placeholder);
    let field = if focus {
        field.focus_when(app.auth.focus).autofocus()
    } else {
        field
    };
    let field = field
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
    let choice = app.auth_choice();
    let name = choice.as_ref().map(|choice| choice.name.clone());
    let width = form_cells(app).saturating_sub(8);
    let mut nodes = vec![
        line(format!("Paste the {} API key.", name.unwrap_or_default())),
        field(app, "paste or type the api key", true, true),
    ];
    nodes.extend(
        wrapped(note, width)
            .into_iter()
            .map(|part| notice(part, Tone::Error)),
    );
    nodes
}

fn signing(app: &App, name: &str, lines: [String; 4], asking: bool) -> Vec<NodeBuilder> {
    let [text, url, code, note] = lines;
    let wide = form_cells(app).saturating_sub(8);
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
        nodes.push(field(
            app,
            "Paste the code if the browser shows one",
            false,
            true,
        ));
    }
    if !note.is_empty() {
        nodes.extend(
            wrapped(&note, wide)
                .into_iter()
                .map(|part| notice(part, Tone::Error)),
        );
        return nodes;
    }
    let clock = app.clone();
    let owner = name.to_string();
    nodes.push(container().children_reactive(move || {
        let spin = sigil::widgets::spinner_frame(clock.ticks.get());
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
    nodes
}
