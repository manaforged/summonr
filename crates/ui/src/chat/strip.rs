use std::collections::HashSet;

use sigil::prelude::Space;
use sigil::prelude::{ControlSize, NodeBuilder, Token, Widget, button, container};

use super::{Agent, App, Model, Speed, canvas, entry, todos};
use crate::model::StatusView;
use crate::text_style::TextStyle;
use crate::ui::{Dialog, Hints, Key, Size};
use summonr_agent::mail::Presence;
use summonr_agent::prefs::Hotkey;

pub(crate) mod bar;
mod details;

pub(crate) use bar::{bar, rows};

fn children(model: &Model) -> impl Iterator<Item = &Agent> {
    model
        .agents
        .iter()
        .filter(|agent| !model.agents_hidden.contains(&agent.id))
}

fn peers(model: &Model) -> impl Iterator<Item = &summonr_agent::mail::Agent> {
    let children: HashSet<&str> = model.agents.iter().map(|agent| agent.id.as_str()).collect();
    model.peers.iter().filter(move |peer| {
        peer.parent.as_deref() == Some(model.session.as_str())
            && !model.agents_hidden.contains(&peer.id)
            && !children.contains(summonr_agent::child::tag(&peer.id).as_str())
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

pub(crate) fn working(model: &Model) -> bool {
    model.busy_since.is_some() || counts(model).1 > 0
}

fn text(body: impl Into<sigil::prelude::TextContent>, token: Token) -> sigil::prelude::TextBuilder {
    TextStyle::xs(token).on(sigil::prelude::text(body).nowrap())
}

pub(crate) fn sheet(app: App) -> NodeBuilder {
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
            .key(Key::Esc, "close")
            .key(Key::Hotkey(Hotkey::Agents), "toggle agents"),
    )
    .child(container().children_reactive(move || {
        let (current, task_count, agent_count) = tabs_view.get();
        let mut tabs = sigil::prelude::row()
            .align_items(sigil::prelude::AlignItems::Stretch)
            .fill_width()
            .gap(Space::Sm);
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
    pub(crate) fn show_status(&self, view: StatusView) {
        self.model_mut(|model| model.status_view = view);
        self.status_list.select(0);
        self.status_list.scroll.set(0.0);
        self.status_detail.set(None);
        self.refresh();
    }

    pub(crate) fn toggle_agents(&self) {
        let view = if self.model.peek().status_view == StatusView::Agents {
            StatusView::Closed
        } else {
            StatusView::Agents
        };
        self.show_status(view);
    }

    pub(crate) fn toggle_todos(&self) {
        let view = if self.model.peek().status_view == StatusView::Tasks {
            StatusView::Closed
        } else {
            StatusView::Tasks
        };
        self.show_status(view);
    }

    pub(crate) fn status_sheet_open(&self) -> sigil::prelude::Signal<bool> {
        let model = self.model;
        sigil::reactive::derived(move || model.with(|model| model.status_view.open()))
    }

    pub(crate) fn close_status_sheet(&self) {
        self.show_status(StatusView::Closed);
    }

    pub(crate) fn step_status(&self, _delta: isize) {
        let next = match self.model.peek().status_view {
            StatusView::Tasks => StatusView::Agents,
            StatusView::Agents | StatusView::Closed => StatusView::Tasks,
        };
        self.show_status(next);
    }

    pub(crate) fn move_status(&self, delta: isize) {
        self.status_list.shift(
            delta,
            details::rows(self).len(),
            sigil::widgets::CursorEdge::Clamp,
        );
    }

    pub(crate) fn expand_status(&self) {
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
