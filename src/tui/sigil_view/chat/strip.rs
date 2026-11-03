use std::time::Instant;

use sigil::prelude::{
    AlignItems, ControlSize, LayoutStyle, NodeBuilder, Token, Widget, button, container,
};

use super::panes::ledger::Mark;
use super::{Agent, App, Model, Speed, canvas, entry, todos};
use crate::mail::Presence;
use crate::tui::Activity;
use crate::tui::sigil_view::model::StatusView;
use crate::tui::sigil_view::text_style::TextStyle;
use crate::tui::sigil_view::ui::{Dialog, Hints, Key, Size};

mod details;

fn children(model: &Model) -> impl Iterator<Item = &Agent> {
    model
        .agents
        .iter()
        .filter(|agent| !model.agents_hidden.contains(&agent.id))
}

fn peers(model: &Model) -> impl Iterator<Item = &crate::mail::Agent> {
    model.peers.iter().filter(|peer| {
        peer.parent.as_deref() == Some(model.session.as_str())
            && !model.agents_hidden.contains(&peer.id)
    })
}

fn counts(model: &Model) -> (usize, usize, usize, usize) {
    let mut counts = (0, 0, 0, 0);
    for agent in children(model) {
        counts.0 += 1;
        counts.1 += usize::from(!agent.done);
        counts.2 += usize::from(agent.failed);
    }
    for peer in peers(model) {
        counts.0 += 1;
        counts.1 += usize::from(peer.status == Presence::Working);
        counts.3 += usize::from(peer.status == Presence::Unknown);
    }
    counts
}

pub(in crate::tui::sigil_view) fn working(model: &Model) -> bool {
    model.busy_since.is_some() || counts(model).1 > 0
}

#[derive(Clone, PartialEq)]
struct Status {
    busy: bool,
    mark: &'static str,
    state: String,
    speed: String,
    tasks: String,
    agents: String,
    warning: String,
}

fn status(app: &App) -> Option<Status> {
    let model = app.model.get();
    let (total, running, failed, unknown) = counts(&model);
    let (done, tasks) = todos::tally(&model.ledger);
    let current = todos::current(&model);
    let waiting = app.approval.get().is_some() || app.form.get().is_some();
    let blocked = model
        .ledger
        .iter()
        .filter(|row| row.mark == Mark::Blocked)
        .count();
    if model.busy_since.is_none() && total == 0 && tasks == 0 && !waiting {
        return None;
    }
    let busy = !waiting && working(&model);
    let state = if waiting {
        "Waiting for input".to_string()
    } else if let Some((activity, _)) = model.busy_since {
        current.map_or_else(
            || activity.label().to_string(),
            |row| match row.mark {
                Mark::Active => row.text.clone(),
                Mark::Blocked => "Blocked".to_string(),
                Mark::Done | Mark::Pending | Mark::Plain => activity.label().to_string(),
            },
        )
    } else if running > 0 {
        "Agents working".to_string()
    } else if current.is_some_and(|row| row.mark == Mark::Blocked) {
        "Blocked".to_string()
    } else if current.is_some() {
        "Pending".to_string()
    } else if failed > 0 {
        "Agents failed".to_string()
    } else if unknown > 0 {
        "Agent status unknown".to_string()
    } else if peers(&model).next().is_some() {
        "Idle".to_string()
    } else {
        "Complete".to_string()
    };
    let speed = if !waiting
        && model.live_tool.is_none()
        && model
            .busy_since
            .is_some_and(|(activity, _)| activity == Activity::Turn)
    {
        model
            .speed
            .live(Instant::now())
            .map(|rate| format!("~{}", Speed::label(rate)))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let tasks = if tasks == 0 {
        String::new()
    } else if total > 0 || model.busy_since.is_some() {
        format!("Tasks {done}/{tasks}")
    } else {
        current.map_or_else(
            || format!("{done}/{tasks} · Complete"),
            |row| format!("{}/{tasks} · {}", done + 1, row.text),
        )
    };
    let mut warnings = Vec::new();
    if failed > 0 {
        warnings.push(format!("{failed} failed"));
    }
    if unknown > 0 {
        warnings.push(format!("{unknown} unknown"));
    }
    if blocked > 0 {
        warnings.push(format!("{blocked} blocked"));
    }
    let pending = match model.pending.len() {
        0 => String::new(),
        n => format!(" · {n} waiting"),
    };
    Some(Status {
        busy,
        mark: if waiting || failed + unknown + blocked > 0 {
            "!"
        } else if current.is_some() {
            "○"
        } else {
            "✓"
        },
        state: format!("{state}{pending}"),
        speed,
        tasks,
        agents: if total > 0 {
            summonr_core::text::plural(total, "agent")
        } else {
            String::new()
        },
        warning: warnings.join(" · "),
    })
}

pub(in crate::tui::sigil_view) fn rows(app: &App) -> usize {
    usize::from(status(app).is_some())
}

fn text(body: impl Into<sigil::prelude::TextContent>, token: Token) -> sigil::prelude::TextBuilder {
    TextStyle::xs(token).on(sigil::prelude::text(body).nowrap())
}

pub(in crate::tui::sigil_view) fn bar(app: App, width: usize) -> NodeBuilder {
    let paint = app.clone();
    let view = sigil::reactive::derived(move || {
        paint.ticks.with(|_| ());
        status(&paint)
    });
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
            let warning_width = super::cell_width(&status.warning);
            let agent_width = super::cell_width(&status.agents);
            let speed_width = if width >= 60 {
                super::cell_width(&status.speed)
            } else {
                0
            };
            let task_width = super::cell_width(&status.tasks).min(width / 2);
            let label_width =
                width.saturating_sub(warning_width + agent_width + task_width + speed_width + 12);
            let mut row = container().layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .height(canvas::cell().h)
                    .min_size(0.0, 0.0)
                    .gap(canvas::cell().w)
                    .align_items(AlignItems::Center),
            );
            row = row.child(if status.busy {
                match app.model.get().prefs.spinner {
                    crate::prefs::Spinner::Dots => text(
                        sigil::widgets::spinner_frame(app.ticks.get()),
                        Token::Accent11,
                    ),
                    crate::prefs::Spinner::Symbols => entry::loading_mark(app.ticks),
                }
            } else {
                text(
                    status.mark,
                    if status.warning.is_empty() {
                        Token::Gray10
                    } else {
                        Token::Error11
                    },
                )
            });
            if label_width > 0 {
                row = row.child(
                    text(
                        crate::display::ellipsize(&status.state, label_width),
                        Token::Gray11,
                    )
                    .ellipsize()
                    .layout(LayoutStyle::block().shrink(1.0).min_size(0.0, 0.0)),
                );
            }
            if !status.speed.is_empty() && width >= 60 {
                row = row.child(text(status.speed, Token::Gray10));
            }
            if !status.tasks.is_empty() {
                let open = app.clone();
                let label = crate::display::ellipsize(&status.tasks, task_width);
                row = row.child(
                    button(label)
                        .ghost()
                        .size(ControlSize::Compact)
                        .accessible(format!("{}; open task details", status.tasks))
                        .activate(move || open.show_status(StatusView::Tasks)),
                );
            }
            if !status.agents.is_empty() {
                let open = app.clone();
                row = row.child(
                    button(status.agents)
                        .ghost()
                        .size(ControlSize::Compact)
                        .accessible("Open agent details")
                        .activate(move || open.show_status(StatusView::Agents)),
                );
            }
            if !status.warning.is_empty() {
                row = row.child(text(status.warning, Token::Error11));
            }
            vec![row]
        })
}

pub(in crate::tui::sigil_view) fn sheet(app: App) -> NodeBuilder {
    let paint = app.clone();
    let keys = app.model.peek().prefs.hotkeys.clone();
    let viewport = app.viewport.peek();
    let open = app.status_sheet_open();
    let update = app.clone();
    let tabs_view = sigil::reactive::derived(move || {
        update.model.with(|model| {
            (
                model.status_view,
                todos::tally(&model.ledger),
                counts(model).0,
            )
        })
    });
    Dialog::new(
        Size::Reading,
        "Status",
        Hints::new()
            .key(Key::Fixed("↑/↓"), "select")
            .key(Key::Fixed("tab"), "tasks/agents")
            .key(Key::Enter, "details")
            .key(Key::Esc, "close"),
    )
    .child(container().children_reactive(move || {
        let (current, task_count, agent_count) = tabs_view.get();
        let mut tabs = container().layout(LayoutStyle::flex_row().fill_width().gap(8.0));
        for (tab, label) in [
            (
                StatusView::Tasks,
                format!("Tasks {}/{}", task_count.0, task_count.1),
            ),
            (StatusView::Agents, format!("Agents {agent_count}")),
        ] {
            let show = paint.clone();
            tabs = tabs.child(
                button(label)
                    .ghost()
                    .size(ControlSize::Compact)
                    .tone(if current == tab {
                        sigil::prelude::Tone::Info
                    } else {
                        sigil::prelude::Tone::Neutral
                    })
                    .activate(move || show.show_status(tab)),
            );
        }
        vec![tabs, details::list(paint.clone())]
    }))
    .build(open, viewport, &keys)
}

impl App {
    pub(in crate::tui::sigil_view) fn show_status(&self, view: StatusView) {
        self.model_mut(|model| model.status_view = view);
        self.status_list.select(0);
        self.status_list.scroll.set(0.0);
        self.status_detail.set(None);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_agents(&self) {
        let view = if self.model.peek().status_view == StatusView::Agents {
            StatusView::Closed
        } else {
            StatusView::Agents
        };
        self.show_status(view);
    }

    pub(in crate::tui::sigil_view) fn toggle_todos(&self) {
        let view = if self.model.peek().status_view == StatusView::Tasks {
            StatusView::Closed
        } else {
            StatusView::Tasks
        };
        self.show_status(view);
    }

    pub(in crate::tui::sigil_view) fn status_sheet_open(&self) -> sigil::prelude::Signal<bool> {
        let model = self.model;
        sigil::reactive::derived(move || model.with(|model| model.status_view.open()))
    }

    pub(in crate::tui::sigil_view) fn close_status_sheet(&self) {
        self.show_status(StatusView::Closed);
    }

    pub(in crate::tui::sigil_view) fn step_status(&self, _delta: isize) {
        let next = match self.model.peek().status_view {
            StatusView::Tasks => StatusView::Agents,
            StatusView::Agents | StatusView::Closed => StatusView::Tasks,
        };
        self.show_status(next);
    }

    pub(in crate::tui::sigil_view) fn move_status(&self, delta: isize) {
        self.status_list.shift(
            delta,
            details::rows(self).len(),
            sigil::widgets::CursorEdge::Clamp,
        );
    }

    pub(in crate::tui::sigil_view) fn expand_status(&self) {
        let rows = details::rows(self);
        if let Some(row) = rows.get(self.status_list.cursor.peek()) {
            let id = row.id.clone();
            self.status_detail.update(|shown| {
                *shown = if shown.as_ref() == Some(&id) {
                    None
                } else {
                    Some(id)
                };
            });
        }
    }
}
