use sigil::prelude::{
    AlignItems, Fill, Geometry, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder,
    NodeEventsExt, NodeStyleExt, PointerEvent, container,
};

use super::super::{App, Call, Entry};
use super::{COMPACT_ROWS, output_block, patch_of, patch_path, patch_view, preview, wrap};
use crate::prefs::{DiffLayout, ToolOutput};
use crate::tui::ToolState;

fn glyph(state: &ToolState) -> (&'static str, &'static str) {
    match state {
        ToolState::Running => ("○", "text-xs text-gray-11"),
        ToolState::Ok => ("✓", "text-xs text-success-11"),
        ToolState::Failed => ("✗", "text-xs text-error-11"),
    }
}

fn header(name: &str, arg: &str, state: &ToolState) -> NodeBuilder {
    let (mark, tone) = glyph(state);
    let (name, arg) = arg
        .strip_prefix("search ")
        .filter(|rest| rest.starts_with('"'))
        .map_or((name, arg), |rest| ("search", rest));
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(
            sigil::kernel::text(mark)
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class(tone),
        )
        .child(
            sigil::kernel::text(name.to_string())
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class("text-xs text-gray-12"),
        )
        .child(
            sigil::kernel::text(arg.to_string())
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .ellipsize()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))
                .class("text-xs font-mono text-gray-11"),
        )
}

fn flip(app: &App, index: usize, call: Option<usize>) -> impl Fn(&mut PointerEvent) + 'static {
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

fn call_output(
    name: &str,
    call: &Call,
    full: bool,
    layout: DiffLayout,
    app: &App,
) -> Option<NodeBuilder> {
    let output = preview::plain(&call.output);
    let failed = matches!(call.state, ToolState::Failed);
    let running = matches!(call.state, ToolState::Running);
    let Some(patch) = patch_of(&output) else {
        let shown = preview::text(full, name, &output, failed, running);
        return (!shown.is_empty()).then(|| output_block(shown));
    };
    let path = patch_path(&output).unwrap_or_else(|| call.arg.clone());
    let status = match call.state {
        ToolState::Running => "editing",
        ToolState::Ok => "✓ Applied",
        ToolState::Failed => "✗ Failed",
    };
    let rows = if full { usize::MAX } else { COMPACT_ROWS };
    let view = patch_view(app, patch, layout)
        .bg("field")
        .border_w(0)
        .path(path)
        .max_rows(rows);
    let body = if crate::tools::Builtin::of(name).is_some_and(crate::tools::Builtin::edits) {
        container().child(view.status(status))
    } else {
        container().child(view)
    };
    Some(body)
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
    let full = call.expanded || tools == ToolOutput::Full;
    let card = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .pointer_up(flip(app, index, at))
        .child(header(name, &call.arg, &call.state));
    match call_output(name, call, full, layout, app) {
        Some(body) => card.child(body),
        None => card,
    }
}

fn group_header(name: &str, calls: &[Call]) -> NodeBuilder {
    let state = if calls
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
    };
    let (mark, tone) = glyph(&state);
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(
            sigil::kernel::text(mark)
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class(tone),
        )
        .child(
            sigil::kernel::text(format!("{name} ×{}", calls.len()))
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class("text-xs text-gray-12"),
        )
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
    let (mark, tone) = glyph(&call.state);
    let summary = preview::summary(
        &preview::plain(&call.output),
        matches!(call.state, ToolState::Failed),
    );
    let row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .pointer_up(flip(app, index, Some(at)))
        .child(
            sigil::kernel::text(mark)
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class(tone),
        )
        .child(
            sigil::kernel::text(call.arg.clone())
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .ellipsize()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))
                .class("text-xs font-mono text-gray-11"),
        )
        .child(
            sigil::kernel::text(summary)
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .ellipsize()
                .class("text-xs text-gray-10"),
        );
    let expanded = call.expanded || tools == ToolOutput::Full;
    let row = wrap(row);
    match expanded
        .then(|| call_output(name, call, true, layout, app))
        .flatten()
    {
        Some(body) => row.child(body),
        None => row,
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
    let mut card = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(group_header(name, calls));
    for (at, call) in calls.iter().enumerate() {
        card = card.child(group_row(name, call, at, tools, layout, app, index));
    }
    card
}
