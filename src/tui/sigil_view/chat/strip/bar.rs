use std::time::Instant;

use sigil::prelude::{
    AlignItems, ControlSize, LayoutStyle, NodeBuilder, Token, Widget, button, container,
};

use super::{App, Model, Speed, StatusView, canvas, counts, entry, text, todos, working};
use crate::tui::Activity;
use crate::tui::sigil_view::model::{Doing, Entry};
use crate::tui::sigil_view::panes::ledger::Mark;

const TASK_COLS: usize = 36;
const TASK_MIN: usize = 12;
const RATE_COLS: usize = 9;
const CLOCK_COLS: usize = 25;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Icon {
    Spinner,
    Waiting,
    Alert,
    Open,
    Clear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Verb,
    Alert,
    Agents,
    Task,
    Queued,
}

#[derive(Clone, PartialEq)]
struct Part {
    rank: Rank,
    text: String,
    tone: Token,
    open: Option<StatusView>,
}

#[derive(Clone, PartialEq)]
struct Status {
    icon: Icon,
    spinner: crate::prefs::Spinner,
    clock: Option<(Instant, bool)>,
    rate: bool,
    parts: Vec<Part>,
}

fn subject(model: &Model) -> Option<String> {
    let Some(Entry::Thinking(text)) = model.entries.last() else {
        return None;
    };
    text.lines().rev().find_map(|line| {
        let title = line.trim().strip_prefix("**")?.strip_suffix("**")?.trim();
        (!title.is_empty()).then(|| format!("{title}…"))
    })
}

fn verb(model: &Model, waiting: bool, running: usize) -> Option<String> {
    if waiting {
        return Some("Waiting for you".to_string());
    }
    let verb = match (model.busy_since.map(|(activity, _)| activity), &model.doing) {
        (Some(Activity::Turn), Some(Doing::Thinking)) => {
            return Some(subject(model).unwrap_or_else(|| "Thinking…".to_string()));
        }
        (Some(Activity::Turn), Some(Doing::Tool { .. })) => "Working…",
        (Some(Activity::Turn), None) => "Writing…",
        (Some(Activity::Login), _) => "Signing in…",
        (Some(Activity::Compact), _) => "Compacting…",
        (Some(Activity::Branch), _) => "Branching…",
        (Some(Activity::Shell), _) => "Running…",
        (None, _) if running > 0 => "Waiting on agents…",
        (None, _) => return None,
    };
    Some(verb.to_string())
}

fn task(model: &Model) -> Option<String> {
    let (done, total) = todos::tally(&model.ledger);
    if total == 0 {
        return None;
    }
    Some(match todos::current(model) {
        Some(row) if matches!(row.mark, Mark::Active | Mark::Blocked) => {
            let body = match row.text.split_once(" · ") {
                Some((_, body)) if row.indent > 0 => body,
                _ => row.text.as_str(),
            };
            let head = format!("{}/{total} ", done + 1);
            let room = TASK_COLS.saturating_sub(head.len());
            format!("{head}{}", crate::display::ellipsize(body, room))
        }
        _ => format!("{done}/{total} tasks"),
    })
}

fn status(app: &App) -> Option<Status> {
    let model = app.model.get();
    let (total, running, failed, unknown) = counts(&model);
    let waiting = app.approval.get().is_some() || app.form.get().is_some();
    let blocked = model
        .ledger
        .iter()
        .filter(|row| row.mark == Mark::Blocked)
        .count();
    if model.busy_since.is_none() && total == 0 && todos::tally(&model.ledger).1 == 0 && !waiting {
        return None;
    }
    let alert = [
        (failed, "failed"),
        (unknown, "unknown"),
        (blocked, "blocked"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{count} {label}"))
    .collect::<Vec<_>>()
    .join(" · ");
    let agents = match (total, running) {
        (0, _) => String::new(),
        (total, running) if running == 0 || running == total => {
            summonr_core::text::plural(total, "agent")
        }
        (total, running) => format!("{running}/{total} agents"),
    };
    let queued = match model.pending.len() {
        0 => String::new(),
        n => format!("{n} queued"),
    };
    let mut parts = Vec::new();
    let mut push = |rank, text: String, tone, open| {
        if !text.is_empty() {
            parts.push(Part {
                rank,
                text,
                tone,
                open,
            });
        }
    };
    let verb_tone = if waiting {
        Token::Accent11
    } else {
        Token::Gray12
    };
    push(
        Rank::Verb,
        verb(&model, waiting, running).unwrap_or_default(),
        verb_tone,
        None,
    );
    push(
        Rank::Task,
        task(&model).unwrap_or_default(),
        Token::Gray11,
        Some(StatusView::Tasks),
    );
    push(
        Rank::Agents,
        agents,
        Token::Gray11,
        Some(StatusView::Agents),
    );
    push(Rank::Alert, alert, Token::Error11, None);
    push(Rank::Queued, queued, Token::Gray10, None);
    let turn = model
        .busy_since
        .is_some_and(|(activity, _)| activity == Activity::Turn);
    Some(Status {
        icon: if waiting {
            Icon::Waiting
        } else if working(&model) {
            Icon::Spinner
        } else if failed + unknown + blocked > 0 {
            Icon::Alert
        } else if todos::current(&model).is_some() {
            Icon::Open
        } else {
            Icon::Clear
        },
        spinner: model.prefs.spinner,
        clock: model.busy_since.map(|(_, since)| (since, turn && !waiting)),
        rate: turn && !waiting && !matches!(model.doing, Some(Doing::Tool { .. })),
        parts,
    })
}

fn cost(part: &Part) -> usize {
    super::super::cell_width(&part.text) + usize::from(part.open.is_some()) * 2 + 3
}

fn fit(mut status: Status, width: usize) -> Status {
    loop {
        let fixed = 2
            + status
                .clock
                .map_or(0, |(_, hint)| if hint { CLOCK_COLS } else { 7 })
            + if status.rate { RATE_COLS + 1 } else { 0 };
        let used = (fixed + status.parts.iter().map(cost).sum::<usize>()).saturating_sub(2);
        if used <= width {
            return status;
        }
        let over = used - width;
        if let Some((_, hint)) = status.clock.as_mut().filter(|(_, hint)| *hint) {
            *hint = false;
            continue;
        }
        if status.rate {
            status.rate = false;
            continue;
        }
        if let Some(task) = status.parts.iter_mut().find(|part| part.rank == Rank::Task) {
            let room = super::super::cell_width(&task.text).saturating_sub(over);
            if room >= TASK_MIN {
                task.text = crate::display::ellipsize(&task.text, room).to_string();
                continue;
            }
        }
        let Some(last) = status
            .parts
            .iter()
            .map(|part| part.rank)
            .filter(|rank| *rank != Rank::Verb)
            .max()
        else {
            return status;
        };
        status.parts.retain(|part| part.rank != last);
    }
}

pub(in crate::tui::sigil_view) fn rows(app: &App) -> usize {
    usize::from(status(app).is_some())
}

fn icon(app: &App, status: &Status) -> sigil::prelude::TextBuilder {
    match status.icon {
        Icon::Spinner => match status.spinner {
            crate::prefs::Spinner::Dots => {
                let ticks = app.ticks;
                text(
                    move || sigil::widgets::spinner_frame(ticks.get()).to_owned(),
                    Token::Accent11,
                )
            }
            crate::prefs::Spinner::Symbols => entry::loading_mark(app.ticks),
        },
        Icon::Waiting => text("?", Token::Accent11),
        Icon::Alert => text("!", Token::Error11),
        Icon::Open => text("○", Token::Gray10),
        Icon::Clear => text("✓", Token::Success11),
    }
}

pub(in crate::tui::sigil_view) fn bar(app: App, width: usize) -> NodeBuilder {
    let paint = app.clone();
    let view = sigil::reactive::derived(move || status(&paint).map(|status| fit(status, width)));
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            let Some(status) = view.get() else {
                return Vec::new();
            };
            let mut row = container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .height(canvas::cell().h)
                        .min_size(0.0, 0.0)
                        .gap(canvas::cell().w)
                        .align_items(AlignItems::Center),
                )
                .child(icon(&app, &status));
            for (index, part) in status.parts.iter().enumerate() {
                if index > 0 {
                    row = row.child(text("·", Token::Gray8));
                }
                row = match part.open {
                    Some(view) => {
                        let open = app.clone();
                        row.child(
                            button(part.text.clone())
                                .ghost()
                                .size(ControlSize::Compact)
                                .layout(LayoutStyle::block().height(canvas::cell().h))
                                .accessible(format!("{}; open details", part.text))
                                .activate(move || open.show_status(view)),
                        )
                    }
                    None => row.child(text(part.text.clone(), part.tone)),
                };
                if index == 0
                    && part.rank == Rank::Verb
                    && let Some((since, hint)) = status.clock
                {
                    let ticks = app.ticks;
                    row = row.child(text(
                        move || {
                            ticks.with(|_| ());
                            let clock = summonr_core::text::duration(since.elapsed().as_secs());
                            if hint {
                                format!("({clock} · esc to interrupt)")
                            } else {
                                format!("({clock})")
                            }
                        },
                        Token::Gray10,
                    ));
                }
            }
            if status.rate {
                let speed = app.clone();
                row = row
                    .child(container().layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0)))
                    .child(text(
                        move || {
                            speed.ticks.with(|_| ());
                            speed.model.with(|model| {
                                model
                                    .speed
                                    .live(Instant::now())
                                    .map(Speed::label)
                                    .unwrap_or_default()
                            })
                        },
                        Token::Gray10,
                    ));
            }
            vec![row]
        })
}

#[cfg(test)]
#[path = "bar_tests.rs"]
mod tests;
