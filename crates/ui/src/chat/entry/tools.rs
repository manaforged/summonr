use sigil::prelude::Space;
use sigil::prelude::TextBuilder;
use sigil::prelude::{AlignItems, MouseButton, NodeBuilder, Paint, PointerEvent, Signal, Token};

use super::super::{App, Call, Entry};
use super::tree::{label, tree};
use super::{COMPACT_ROWS, patch_of, patch_path, patch_view, preview, wrap};
use crate::ToolState;
use crate::text_style::TextStyle;
use sigil::prelude::Widget;
use summonr_agent::prefs::{DiffLayout, ToolOutput};
use summonr_agent::tools::Builtin;

pub(super) type Detail = Option<(String, TextStyle)>;

const LOADING: [&str; 4] = ["✳", "✢", "∗", "✻"];

pub(crate) fn glyph(state: &ToolState) -> (&'static str, TextStyle) {
    match state {
        ToolState::Running => ("○", TextStyle::xs(Token::Gray11)),
        ToolState::Ok => ("●", TextStyle::xs(Token::Success11)),
        ToolState::Failed => ("●", TextStyle::xs(Token::Error11)),
    }
}

pub(crate) fn loading_mark(ticks: Signal<u64>) -> TextBuilder {
    let body = move || {
        let at = usize::try_from(ticks.get() % LOADING.len() as u64).unwrap_or(0);
        LOADING[at].to_owned()
    };
    TextStyle::xs(Token::Accent11).line(body)
}

pub(crate) fn workspace_relative(arg: &str, roots: &[String]) -> String {
    arg.split(' ')
        .map(|word| {
            let (lead, path) = word.split_at(
                word.find(|ch| !matches!(ch, '"' | '\'' | '=' | '(' | '['))
                    .unwrap_or(0),
            );
            roots
                .iter()
                .find_map(|root| {
                    let rest = path.strip_prefix(root.as_str())?;
                    match rest.strip_prefix(['/', std::path::MAIN_SEPARATOR]) {
                        Some("") => Some(format!("{lead}./")),
                        Some(inner) => Some(format!("{lead}{inner}")),
                        None if rest.is_empty()
                            || rest.starts_with(['"', '\'', ')', ']', ',', '.', ':', ';']) =>
                        {
                            Some(format!("{lead}.{rest}"))
                        }
                        None => None,
                    }
                })
                .unwrap_or_else(|| word.to_string())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn detail(name: &str, call: &Call, output: &str) -> Detail {
    match call.state {
        ToolState::Running => None,
        ToolState::Failed => preview::exit(output)
            .map(|code| (format!("exit {code}"), TextStyle::xs(Token::Error11))),
        ToolState::Ok => Builtin::is(name, Builtin::Read)
            .then(|| summonr_agent::tools::read_span(output))
            .flatten()
            .filter(|_| output.starts_with(call.arg.as_str()))
            .map(|(from, to)| (format!("{from}-{to}"), TextStyle::xs(Token::Gray10))),
    }
}

pub(super) fn call_line(verb: &str, arg: &str, detail: Detail, strong: bool) -> NodeBuilder {
    let (head, body) = if strong {
        (
            TextStyle::xs(Token::Gray12).bold(),
            TextStyle::xs(Token::Gray12).mono(),
        )
    } else {
        (
            TextStyle::xs(Token::Gray11),
            TextStyle::xs(Token::Gray11).mono(),
        )
    };
    let mut row = sigil::prelude::row()
        .grow(1.0)
        .gap(Space::Sm)
        .min_width(0.0)
        .min_height(0.0)
        .align_items(AlignItems::Center)
        .child(label(verb, head));
    if !arg.is_empty() {
        row = row.child(
            label(arg, body)
                .ellipsize()
                .shrink(1.0)
                .min_width(0.0)
                .min_height(0.0),
        );
    }
    match detail {
        Some((text, class)) => row.child(label(text, class)),
        None => row,
    }
}

pub(super) fn head_row(
    mark: impl sigil::prelude::IntoNode + 'static,
    words: impl sigil::prelude::IntoNode + 'static,
) -> NodeBuilder {
    sigil::prelude::row()
        .fill_width()
        .gap(Space::Sm)
        .min_width(0.0)
        .min_height(0.0)
        .align_items(AlignItems::Center)
        .child(mark)
        .child(words)
}

pub(super) fn header(state: &ToolState, words: NodeBuilder) -> NodeBuilder {
    let (mark, tone) = glyph(state);
    head_row(label(mark, tone), words)
}

pub(crate) fn flip(app: &App, index: usize, at: usize) -> impl Fn(&mut PointerEvent) + 'static {
    let app = app.clone();
    move |event: &mut PointerEvent| {
        if event.button != Some(MouseButton::Left) {
            return;
        }
        app.model_mut(|model| {
            if let Some(Entry::Tool { calls, .. }) = model.entries.get_mut(index)
                && let Some(call) = calls.get_mut(at)
            {
                call.expanded = !call.expanded;
            }
        });
        event.stop_propagation();
    }
}

pub(crate) fn call_output(
    name: &str,
    call: &Call,
    output: &str,
    full: bool,
    layout: DiffLayout,
    app: &App,
) -> Option<NodeBuilder> {
    let failed = matches!(call.state, ToolState::Failed);
    let running = matches!(call.state, ToolState::Running);
    let patch = call.patch.clone().or_else(|| patch_of(output));
    if patch.is_none()
        && let Some(card) = super::reply::stat_node_of(output)
    {
        return Some(wrap(card));
    }
    let Some(patch) = patch else {
        let ranged =
            Builtin::is(name, Builtin::Read) && summonr_agent::tools::read_span(output).is_some();
        if ranged && !full && !failed {
            return None;
        }
        let shown = preview::text(full, name, output, failed, running);
        return (!shown.is_empty()).then(|| tree(&shown));
    };
    let rows = if full { usize::MAX } else { COMPACT_ROWS };
    let view = patch_view(app, patch, layout, super::Fit::chat(app))
        .background(Token::Field)
        .border_width(0)
        .max_rows(rows);
    let view = if Builtin::of(name).is_some_and(Builtin::edits) {
        view
    } else {
        view.path(patch_path(output).unwrap_or_else(|| call.arg.clone()))
    };
    Some(wrap(view))
}

fn task_tool_card(
    name: &str,
    call: &Call,
    tools: ToolOutput,
    app: &App,
    index: usize,
    at: usize,
) -> Option<NodeBuilder> {
    if !Builtin::is(name, Builtin::Task) {
        return None;
    }
    if let Some(card) = super::children::task_card(app, &call.id) {
        return Some(card);
    }
    let full = call.expanded || tools == ToolOutput::Full;
    let hidden = tools == ToolOutput::Hidden && !call.expanded;
    if let Some(card) = super::children::receipt_card(call, full, hidden, app) {
        return Some(card.pointer_up(flip(app, index, at)));
    }
    (call.arg.is_empty() && !matches!(call.state, ToolState::Failed))
        .then(|| super::children::starting_card(app.ticks))
}

fn resolve_card_output(call: &Call, hidden: bool, full: bool, roots: &[String], app: &App) -> String {
    let resolved =
        (!hidden && full && call.compacted).then(|| call.resolved_output(&app.workspace()));
    let raw_output = resolved.as_deref().unwrap_or(&call.output);
    preview::plain(raw_output)
        .lines()
        .map(|line| workspace_relative(line, roots))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn tool_card(
    name: &str,
    call: &Call,
    tools: ToolOutput,
    layout: DiffLayout,
    app: &App,
    index: usize,
    at: usize,
) -> NodeBuilder {
    if let Some(card) = task_tool_card(name, call, tools, app, index, at) {
        return card;
    }
    let full = call.expanded || tools == ToolOutput::Full;
    let hidden = tools == ToolOutput::Hidden && !call.expanded;
    let roots = app.roots();
    let shown = workspace_relative(&call.arg, &roots);
    let (verb, arg) = (tool_label(name), shown.as_str());
    let output = resolve_card_output(call, hidden, full, &roots, app);
    let detail = detail(name, call, &output);
    let card = sigil::prelude::column()
        .fill_width()
        .gap(Space::None)
        .pointer_up(flip(app, index, at))
        .child(header(&call.state, call_line(verb, arg, detail, true)));
    match (!hidden)
        .then(|| call_output(name, call, &output, full, layout, app))
        .flatten()
    {
        Some(body) => card.child(body),
        None => card,
    }
}

mod group;
pub(super) use group::group_card;

pub(crate) fn tool_label(name: &str) -> &str {
    match summonr_agent::tools::Builtin::of(name) {
        Some(kind) => kind.verb(),
        None => name,
    }
}
