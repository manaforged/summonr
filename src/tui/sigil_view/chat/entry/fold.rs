use std::collections::HashSet;
use std::ops::Range;

use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, Paint,
    PointerEvent, Radius, Signal, Token, container,
};

use super::super::App;
use super::preview;
use super::tools::{call_output, flip, glyph, workspace_relative};
use super::tree::label;
use super::verb::{Step, StepVerb, chip, patch_counts, step_meta};
use crate::tui::ToolState;
use crate::tui::sigil_view::model::Entry;
use crate::tui::sigil_view::text_style::TextStyle;

fn flatten(entries: &[Entry], start: usize) -> Vec<Step<'_>> {
    let mut steps = Vec::new();
    for (offset, entry) in entries.iter().enumerate() {
        let index = start + offset;
        match entry {
            Entry::Tool { name, calls } => {
                for (at, call) in calls.iter().enumerate() {
                    steps.push(Step {
                        name,
                        call,
                        index,
                        at,
                    });
                }
            }
            Entry::User(_)
            | Entry::Thinking(_)
            | Entry::Receipt(_)
            | Entry::Assistant { .. }
            | Entry::Verify { .. }
            | Entry::Mail { .. }
            | Entry::Aborted(_)
            | Entry::System(_)
            | Entry::Error(_)
            | Entry::Chart(_)
            | Entry::Image { .. } => {}
        }
    }
    steps
}

struct Totals {
    steps: usize,
    edited: HashSet<String>,
    adds: usize,
    dels: usize,
    running: bool,
}

fn totals(steps: &[Step<'_>], roots: &[String]) -> Totals {
    let mut sum = Totals {
        steps: steps.len(),
        edited: HashSet::new(),
        adds: 0,
        dels: 0,
        running: false,
    };
    for step in steps {
        sum.running |= matches!(step.call.state, ToolState::Running);
        if step.kind() != StepVerb::Edited {
            continue;
        }
        let path = workspace_relative(&step.call.arg, roots);
        sum.edited.insert(path);
        if let Some((adds, dels)) = patch_counts(step.call) {
            sum.adds += adds;
            sum.dels += dels;
        }
    }
    sum
}

fn summary(
    sum: &Totals,
    worked: Option<u64>,
    unfolded: bool,
    open: Signal<Option<bool>>,
) -> NodeBuilder {
    let mut parts = Vec::new();
    if let Some(secs) = worked {
        let lead = if sum.running {
            "Working for"
        } else {
            "Worked for"
        };
        parts.push(format!("{lead} {}", summonr_core::text::duration(secs)));
    }
    parts.push(summonr_core::text::plural(sum.steps, "step"));
    if !sum.edited.is_empty() {
        parts.push(format!(
            "{} edited",
            summonr_core::text::plural(sum.edited.len(), "file")
        ));
    }
    let mut head = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(10.0)
                .padding_x(14.0)
                .padding_y(10.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .bg(Token::Surface)
        .pointer_up(move |event: &mut PointerEvent| {
            if event.button == Some(MouseButton::Left) {
                open.set(Some(!unfolded));
                event.stop_propagation();
            }
        })
        .child(label(
            if unfolded { "▾" } else { "▸" },
            TextStyle::sm(Token::Gray10),
        ))
        .child(
            label(parts.join(" · "), TextStyle::sm(Token::Gray11))
                .ellipsize()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0)),
        );
    if sum.adds + sum.dels > 0 {
        head = head
            .child(label(
                format!("+{}", sum.adds),
                TextStyle::xs(Token::Success11),
            ))
            .child(label(
                format!("−{}", sum.dels),
                TextStyle::xs(Token::Error11),
            ));
    }
    head
}

fn step_row(step: &Step<'_>, app: &App, layout: crate::prefs::DiffLayout) -> NodeBuilder {
    let kind = step.kind();
    let tone = match step.call.state {
        ToolState::Failed => glyph(&step.call.state).1,
        ToolState::Running | ToolState::Ok => TextStyle::sm(Token::Gray10),
    };
    let mut line = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(10.0)
                .padding_y(5.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .pointer_up(flip(app, step.index, step.at))
        .child(label(kind.icon(), tone))
        .child(label(kind.verb(step.name), TextStyle::sm(Token::Gray11)));
    for item in step.chips(&app.roots) {
        line = line.child(chip(item));
    }
    line = line.child(container().layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0)));
    if let Some((text, text_style)) = step_meta(step) {
        line = line.child(label(text, text_style));
    }
    let column = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .child(line);
    if !step.call.expanded {
        return column;
    }
    let output = preview::plain(&step.call.output);
    match call_output(step.name, step.call, &output, true, layout, app) {
        Some(body) => column.child(body),
        None => column,
    }
}

pub(in crate::tui::sigil_view) fn card(
    app: &App,
    range: Range<usize>,
    unfolded: Option<bool>,
    open: Signal<Option<bool>>,
) -> NodeBuilder {
    app.model.with(|model| {
        let live = model.busy_since.is_some() && range.end == model.entries.len();
        let worked = model
            .busy_since
            .as_ref()
            .filter(|_| live)
            .map(|(_, since)| since.elapsed().as_secs());
        let steps = flatten(
            model.entries.get(range.clone()).unwrap_or_default(),
            range.start,
        );
        let sum = totals(&steps, &app.roots);
        let unfolded = unfolded.unwrap_or(sum.running || worked.is_some());
        let mut card = container()
            .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0))
            .border_w(1)
            .border_color(Token::Border)
            .rounded(Radius::Lg)
            .child(summary(&sum, worked, unfolded, open));
        if unfolded {
            let mut list = container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .padding_x(14.0)
                        .padding_y(6.0)
                        .min_size(0.0, 0.0),
                )
                .border_top(1)
                .border_color(Token::Border);
            for step in &steps {
                list = list.child(step_row(step, app, model.prefs.diff_layout));
            }
            card = card.child(list);
        }
        card
    })
}
