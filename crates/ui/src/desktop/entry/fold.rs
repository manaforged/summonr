use sigil::prelude::Widget;
use std::collections::HashSet;
use std::ops::Range;

use sigil::prelude::{
    AlignItems, MouseButton, NodeBuilder, Paint, PointerEvent, Signal, Space, Token, container,
};

use super::super::App;
use super::verb::{Step, StepVerb, chip, patch_counts, step_meta};
use crate::ToolState;
use crate::chat::entry::preview;
use crate::chat::entry::tools::{call_output, flip, glyph, workspace_relative};
use crate::chat::entry::tree::label;
use crate::model::Entry;
use crate::text_style::TextStyle;
use summonr_agent::prefs::Visibility;

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
            | Entry::Injected(_)
            | Entry::Error(_)
            | Entry::Chart(_)
            | Entry::Image { .. } => {}
        }
    }
    steps
}

struct Totals {
    steps: usize,
    ran: usize,
    read: usize,
    searched: usize,
    other: usize,
    edited: HashSet<String>,
    adds: usize,
    dels: usize,
    running: bool,
}

fn totals(steps: &[Step<'_>], roots: &[String]) -> Totals {
    let mut sum = Totals {
        steps: steps.len(),
        ran: 0,
        read: 0,
        searched: 0,
        other: 0,
        edited: HashSet::new(),
        adds: 0,
        dels: 0,
        running: false,
    };
    for step in steps {
        sum.running |= matches!(step.call.state, ToolState::Running);
        match step.kind() {
            StepVerb::Edited => {}
            StepVerb::Ran => {
                sum.ran += 1;
                continue;
            }
            StepVerb::Explored => {
                sum.read += 1;
                continue;
            }
            StepVerb::Searched => {
                sum.searched += 1;
                continue;
            }
            StepVerb::Fetched
            | StepVerb::Inspected
            | StepVerb::Loaded
            | StepVerb::Planned
            | StepVerb::Delegated
            | StepVerb::Called => {
                sum.other += 1;
                continue;
            }
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
    let window = crate::canvas::is_window();
    if let Some(secs) = worked.filter(|_| !(window && sum.running)) {
        let lead = if sum.running {
            "Working for"
        } else {
            "Worked for"
        };
        parts.push(format!("{lead} {}", summonr_core::text::duration(secs)));
    }
    parts.push(plain(sum));
    let mut head = sigil::prelude::row()
        .role(sigil::prelude::Role::Button)
        .label(parts.join(" · "))
        .fill_width()
        .gap(if window { Space::Xs } else { Space::Sm })
        .padding_y(Space::raw(
            6.0,
            "a folded summary row sits tighter than a step",
        ))
        .min_width(0.0)
        .min_height(0.0)
        .align_items(AlignItems::Center)
        .pointer_up(move |event: &mut PointerEvent| {
            if event.button == Some(MouseButton::Left) {
                open.set(Some(!unfolded));
                event.stop_propagation();
            }
        });
    if !window {
        head = head.child(label(
            if unfolded { "▾" } else { "▸" },
            TextStyle::sm(Token::Gray10),
        ));
    }
    let grow = if window { 0.0 } else { 1.0 };
    head = head.child(
        label(parts.join(" · "), TextStyle::sm(Token::Gray11))
            .ellipsize()
            .grow(grow)
            .shrink(1.0)
            .min_width(0.0)
            .min_height(0.0),
    );
    if window {
        let mark = if unfolded {
            sigil::icons::Icon::ChevronDown
        } else {
            sigil::icons::Icon::ChevronRight
        };
        return head.child(sigil::widgets::icon(mark).px(14.0));
    }
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

fn plain(sum: &Totals) -> String {
    use summonr_core::text::plural;
    let edited = sum.edited.len();
    let mut said = Vec::new();
    if edited > 0 {
        said.push(format!("edited {}", plural(edited, "file")));
    }
    if sum.ran > 0 {
        said.push(format!("ran {}", plural(sum.ran, "command")));
    }
    if sum.read > 0 {
        said.push(format!("looked at {}", plural(sum.read, "file")));
    }
    if sum.searched > 0 {
        said.push(format!("ran {}", plural(sum.searched, "search")));
    }
    if sum.other > 0 {
        let noun = if said.is_empty() {
            "step"
        } else {
            "other step"
        };
        said.push(plural(sum.other, noun));
    }
    if said.is_empty() {
        said.push(plural(sum.steps, "step"));
    }
    let text = said.join(", ");
    let mut chars = text.chars();
    chars.next().map_or(text.clone(), |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// The step note while the step waits for the user's OK.
const WAITING: &str = "Needs you";

fn step_row(step: &Step<'_>, app: &App, layout: summonr_agent::prefs::DiffLayout) -> NodeBuilder {
    let kind = step.kind();
    let tone = match step.call.state {
        ToolState::Failed => glyph(&step.call.state).1,
        ToolState::Running | ToolState::Ok => TextStyle::sm(Token::Gray10),
    };
    let mut line = sigil::prelude::row()
        .fill_width()
        .gap(Space::raw(
            10.0,
            "the step icon sits closer than a scale step",
        ))
        .padding_y(Space::raw(5.0, "step rows pack tighter than a scale step"))
        .min_width(0.0)
        .min_height(0.0)
        .align_items(AlignItems::Center)
        .pointer_up(flip(app, step.index, step.at));
    let window = crate::canvas::is_window();
    if !window {
        line = line.child(label(kind.icon(), tone));
    }
    line = line.child(label(kind.verb(step.name), TextStyle::sm(Token::Gray11)));
    for item in step.chips(&app.roots()) {
        line = line.child(chip(item));
    }
    line = line.child(container().grow(1.0).min_width(0.0).min_height(0.0));
    let meta = step_meta(step);
    if window && matches!(step.call.state, ToolState::Running) {
        let watch = app.clone();
        line = line.child(container().children_reactive(move || {
            let (text, style) = if watch.approval.get().is_some() {
                (WAITING.to_string(), TextStyle::xs(Token::Warning11))
            } else {
                meta.clone()
                    .unwrap_or_else(|| (String::new(), TextStyle::xs(Token::Gray10)))
            };
            vec![label(text, style)]
        }));
    } else if let Some((text, text_style)) = meta {
        line = line.child(label(text, text_style));
    }
    let column = sigil::prelude::column()
        .fill_width()
        .gap(Space::None)
        .child(line);
    if !step.call.expanded {
        return column;
    }
    let resolved = step.call.resolved_output(&app.workspace());
    let output = preview::plain(&resolved);
    match call_output(step.name, step.call, &output, true, layout, app) {
        Some(body) => column.child(body),
        None => column,
    }
}

pub(crate) fn card(
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
        let sum = totals(&steps, &app.roots());
        let every = model.prefs.tools == summonr_agent::prefs::ToolOutput::Full;
        let unfolded = unfolded.unwrap_or(sum.running || worked.is_some() || every);
        let mut card = sigil::prelude::column()
            .gap(sigil::prelude::Space::None)
            .fill_width()
            .min_width(0.0)
            .min_height(0.0)
            .child(summary(&sum, worked, unfolded, open));
        if unfolded {
            let mut list = sigil::prelude::column()
                .gap(sigil::prelude::Space::None)
                .fill_width()
                .padding_left(Space::raw(
                    14.0,
                    "indents the steps under the summary glyph",
                ))
                .padding_y(Space::raw(6.0, "matches the folded summary row"))
                .min_width(0.0)
                .min_height(0.0)
                .border_left(1)
                .border_color(Token::BorderSubtle);
            let last = model.entries.len().saturating_sub(1);
            for index in range.clone() {
                if let Some(Entry::Thinking(text)) = model.entries.get(index)
                    && model.prefs.reasoning != Visibility::Off
                {
                    let now = live && index == last;
                    list = list.child(super::message::thinking(
                        text,
                        model.prefs.reasoning,
                        now,
                        app,
                    ));
                }
                for step in steps.iter().filter(|step| step.index == index) {
                    list = list.child(step_row(step, app, model.prefs.diff_layout));
                }
            }
            card = card.child(list);
        }
        card
    })
}
