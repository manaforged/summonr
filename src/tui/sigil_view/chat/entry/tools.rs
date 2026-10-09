use sigil::prelude::{
    AlignItems, IntoNode, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt,
    Paint, PointerEvent, Token, container,
};

use super::super::{App, Call, Entry};
use super::tree::{branch, label, tree};
use super::{COMPACT_ROWS, patch_of, patch_path, patch_view, preview};
use crate::prefs::{DiffLayout, ToolOutput};
use crate::tools::Builtin;
use crate::tui::ToolState;
use crate::tui::sigil_view::look::Look;

pub(super) type Detail = Option<(String, Look)>;

pub(in crate::tui::sigil_view) fn glyph(state: &ToolState) -> (&'static str, Look) {
    match state {
        ToolState::Running => ("○", Look::xs(Token::Gray11)),
        ToolState::Ok => ("●", Look::xs(Token::Success11)),
        ToolState::Failed => ("●", Look::xs(Token::Error11)),
    }
}

pub(in crate::tui::sigil_view) fn workspace_relative(arg: &str, roots: &[String]) -> String {
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

fn verb<'a>(name: &'a str, arg: &'a str) -> (&'a str, &'a str) {
    (Builtin::of(name).map_or(name, |kind| kind.verb()), arg)
}

fn detail(name: &str, call: &Call, output: &str) -> Detail {
    match call.state {
        ToolState::Running => None,
        ToolState::Failed => {
            preview::exit(output).map(|code| (format!("exit {code}"), Look::xs(Token::Error11)))
        }
        ToolState::Ok => Builtin::is(name, Builtin::Read)
            .then(|| crate::tools::read_span(output))
            .flatten()
            .filter(|_| output.starts_with(call.arg.as_str()))
            .map(|(from, to)| (format!("{from}-{to}"), Look::xs(Token::Gray10))),
    }
}

pub(super) fn call_line(verb: &str, arg: &str, detail: Detail, strong: bool) -> NodeBuilder {
    let (head, body) = if strong {
        (
            Look::xs(Token::Gray12).bold(),
            Look::xs(Token::Gray12).mono(),
        )
    } else {
        (Look::xs(Token::Gray11), Look::xs(Token::Gray11).mono())
    };
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .grow(1.0)
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(label(verb, head));
    if !arg.is_empty() {
        row = row.child(
            label(arg, body)
                .ellipsize()
                .layout(LayoutStyle::block().shrink(1.0).min_size(0.0, 0.0)),
        );
    }
    match detail {
        Some((text, class)) => row.child(label(text, class)),
        None => row,
    }
}

pub(super) fn header(state: &ToolState, words: NodeBuilder) -> NodeBuilder {
    let (mark, tone) = glyph(state);
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(label(mark, tone))
        .child(words)
}

pub(in crate::tui::sigil_view) fn flip(
    app: &App,
    index: usize,
    call: Option<usize>,
) -> impl Fn(&mut PointerEvent) + 'static {
    let app = app.clone();
    move |event: &mut PointerEvent| {
        if event.button != Some(MouseButton::Left) {
            return;
        }
        app.model_mut(|model| {
            match call {
                None => {
                    if let Some(Entry::Tool { expanded, .. }) = model.entries.get_mut(index) {
                        *expanded = !*expanded;
                    }
                }
                Some(at) => {
                    if let Some(Entry::ToolGroup { calls, .. }) = model.entries.get_mut(index)
                        && let Some(call) = calls.get_mut(at)
                    {
                        call.expanded = !call.expanded;
                    }
                }
            }
            model.entries.invalidate();
        });
        event.stop_propagation();
    }
}

pub(in crate::tui::sigil_view) fn call_output(
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
        return Some(framed(card));
    }
    let Some(patch) = patch else {
        let ranged = Builtin::is(name, Builtin::Read) && crate::tools::read_span(output).is_some();
        if ranged && !full && !failed {
            return None;
        }
        let shown = preview::text(full, name, output, failed, running);
        return (!shown.is_empty()).then(|| tree(&shown));
    };
    let rows = if full { usize::MAX } else { COMPACT_ROWS };
    let view = patch_view(app, patch, layout, app.chat_cols())
        .bg(Token::Field)
        .border_w(0)
        .max_rows(rows);
    let view = if Builtin::of(name).is_some_and(Builtin::edits) {
        view
    } else {
        view.path(patch_path(output).unwrap_or_else(|| call.arg.clone()))
    };
    Some(framed(view))
}

fn framed(child: impl IntoNode + 'static) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .min_size(0.0, 0.0)
                .padding_bottom(crate::tui::sigil_view::canvas::GUTTER),
        )
        .child(child)
}

pub(super) fn tool_card(
    name: &str,
    call: &Call,
    tools: ToolOutput,
    layout: DiffLayout,
    app: &App,
    index: usize,
    at: Option<usize>,
) -> NodeBuilder {
    if Builtin::is(name, Builtin::Task)
        && let Some(card) = super::children::task_card(app, &call.id)
    {
        return card;
    }
    let full = call.expanded || tools == ToolOutput::Full;
    let hidden = tools == ToolOutput::Hidden && !call.expanded;
    if Builtin::is(name, Builtin::Task)
        && let Some(card) = super::children::receipt_card(call, full, hidden, app)
    {
        return card.pointer_up(flip(app, index, at));
    }
    let shown = workspace_relative(&call.arg, &app.roots);
    let (verb, arg) = verb(name, &shown);
    let output = preview::plain(&call.output);
    let detail = detail(name, call, &output);
    let card = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
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

fn group_state(calls: &[Call]) -> ToolState {
    if calls
        .iter()
        .any(|call| matches!(call.state, ToolState::Running))
    {
        ToolState::Running
    } else if calls
        .iter()
        .any(|call| matches!(call.state, ToolState::Failed))
    {
        ToolState::Failed
    } else {
        ToolState::Ok
    }
}

fn group_title<'a>(name: &'a str, calls: &[Call]) -> (&'a str, String) {
    if !Builtin::is(name, Builtin::Read) {
        let verb = Builtin::of(name).map_or(name, |kind| kind.verb());
        return (verb, format!("×{}", calls.len()));
    }
    ("Explored", crate::text::plural(calls.len(), "file"))
}

fn group_row(
    name: &str,
    call: &Call,
    at: usize,
    tools: ToolOutput,
    layout: DiffLayout,
    app: &App,
    index: usize,
) -> NodeBuilder {
    let output = preview::plain(&call.output);
    let shown = workspace_relative(&call.arg, &app.roots);
    let (verb, arg) = verb(name, &shown);
    let detail = match call.state {
        ToolState::Failed => Some((preview::summary(&output, true), Look::xs(Token::Error11))),
        _ => detail(name, call, &output),
    };
    let line =
        branch(at == 0, call_line(verb, arg, detail, false)).pointer_up(flip(app, index, Some(at)));
    let expanded = call.expanded || tools == ToolOutput::Full;
    let column = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(line);
    match expanded
        .then(|| call_output(name, call, &output, true, layout, app))
        .flatten()
    {
        Some(body) => column.child(body),
        None => column,
    }
}

pub(super) fn group_card(
    name: &str,
    calls: &[Call],
    tools: ToolOutput,
    layout: DiffLayout,
    app: &App,
    index: usize,
) -> NodeBuilder {
    let (title, counts) = group_title(name, calls);
    let mut card = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(header(
            &group_state(calls),
            call_line(title, "", Some((counts, Look::xs(Token::Gray10))), true),
        ));
    for (at, call) in calls.iter().enumerate() {
        card = card.child(group_row(name, call, at, tools, layout, app, index));
    }
    card
}
