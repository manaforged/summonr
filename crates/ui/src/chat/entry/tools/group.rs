use sigil::prelude::{MouseButton, NodeBuilder, PointerEvent, Space, Token, Widget};
use summonr_agent::prefs::{DiffLayout, ToolOutput};
use summonr_agent::tools::Builtin;

use super::super::preview;
use super::super::tree::{branch, label};
use super::{call_line, call_output, detail, flip, header, tool_label, workspace_relative};
use crate::ToolState;
use crate::app::App;
use crate::model::Call;
use crate::text_style::TextStyle;

const GROUP_COMPACT_MAX: usize = 8;
const GROUP_TAIL: usize = 5;

struct RowCtx<'a> {
    name: &'a str,
    tools: ToolOutput,
    layout: DiffLayout,
    app: &'a App,
    index: usize,
    roots: &'a [String],
    unfolded: bool,
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
        return (tool_label(name), format!("×{}", calls.len()));
    }
    ("Explored", summonr_core::text::plural(calls.len(), "file"))
}

fn show_group_row(
    at: usize,
    call: &Call,
    len: usize,
    tools: ToolOutput,
    unfolded: bool,
) -> bool {
    let important = call.expanded || !matches!(call.state, ToolState::Ok);
    match tools {
        ToolOutput::Hidden => important,
        ToolOutput::Compact if !unfolded && len > GROUP_COMPACT_MAX => {
            important || at == 0 || at + GROUP_TAIL >= len
        }
        ToolOutput::Compact | ToolOutput::Full => true,
    }
}

fn unfold(app: &App, index: usize) -> impl Fn(&mut PointerEvent) + 'static {
    let app = app.clone();
    move |event: &mut PointerEvent| {
        if event.button != Some(MouseButton::Left) {
            return;
        }
        app.model_mut(|model| model.entries.unfold(index));
        event.stop_propagation();
    }
}

fn fold_row(ctx: &RowCtx<'_>, count: usize, first: bool) -> NodeBuilder {
    let noun = if Builtin::is(ctx.name, Builtin::Read) {
        "file"
    } else {
        "call"
    };
    let text = format!("… +{}", summonr_core::text::plural(count, noun));
    branch(first, label(text, TextStyle::xs(Token::Gray10)))
        .pointer_up(unfold(ctx.app, ctx.index))
}

fn group_row(ctx: &RowCtx<'_>, call: &Call, at: usize, first: bool) -> NodeBuilder {
    let expanded = call.expanded || ctx.tools == ToolOutput::Full;
    let resolved = (expanded && call.compacted).then(|| call.resolved_output(&ctx.app.workspace()));
    let raw_output = resolved.as_deref().unwrap_or(&call.output);
    let output = preview::plain(raw_output);
    let shown = workspace_relative(&call.arg, ctx.roots);
    let (verb, arg) = (tool_label(ctx.name), shown.as_str());
    let detail = match call.state {
        ToolState::Failed => Some((
            preview::summary(&output, true),
            TextStyle::xs(Token::Error11),
        )),
        ToolState::Running | ToolState::Ok => detail(ctx.name, call, &output),
    };
    let line = branch(first, call_line(verb, arg, detail, false))
        .pointer_up(flip(ctx.app, ctx.index, at));
    match expanded
        .then(|| call_output(ctx.name, call, &output, true, ctx.layout, ctx.app))
        .flatten()
    {
        Some(body) => sigil::prelude::column()
            .fill_width()
            .gap(Space::None)
            .child(line)
            .child(body),
        None => line,
    }
}

fn group_rows(ctx: &RowCtx<'_>, calls: &[Call]) -> Vec<NodeBuilder> {
    let len = calls.len();
    let cap = if ctx.tools == ToolOutput::Compact && !ctx.unfolded && len > GROUP_COMPACT_MAX {
        GROUP_TAIL + 3
    } else {
        len
    };
    let mut rows = Vec::with_capacity(cap);
    let mut hidden_run = 0_usize;
    let mut first = true;
    for (at, call) in calls.iter().enumerate() {
        if !show_group_row(at, call, len, ctx.tools, ctx.unfolded) {
            hidden_run += 1;
            continue;
        }
        if hidden_run > 0 && ctx.tools == ToolOutput::Compact {
            rows.push(fold_row(ctx, hidden_run, first));
            first = false;
        }
        hidden_run = 0;
        rows.push(group_row(ctx, call, at, first));
        first = false;
    }
    if hidden_run > 0 && ctx.tools == ToolOutput::Compact {
        rows.push(fold_row(ctx, hidden_run, first));
    }
    rows
}

pub(in super::super) fn group_card(
    name: &str,
    calls: &[Call],
    tools: ToolOutput,
    layout: DiffLayout,
    app: &App,
    index: usize,
) -> NodeBuilder {
    let (title, counts) = group_title(name, calls);
    let mut card = sigil::prelude::column()
        .fill_width()
        .gap(Space::None)
        .child(header(
            &group_state(calls),
            call_line(
                title,
                "",
                Some((counts, TextStyle::xs(Token::Gray10))),
                true,
            ),
        ));
    let unfolded = tools == ToolOutput::Compact
        && calls.len() > GROUP_COMPACT_MAX
        && app.model.peek().entries.unfolded(index);
    let roots = app.roots();
    let ctx = RowCtx {
        name,
        tools,
        layout,
        app,
        index,
        roots: &roots,
        unfolded,
    };
    for row in group_rows(&ctx, calls) {
        card = card.child(row);
    }
    card
}
