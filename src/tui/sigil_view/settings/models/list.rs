use std::collections::BTreeMap;
use std::rc::Rc;

use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, ListRowSlots, NodeBuilder, NodeEventsExt, Tone, WidgetLayoutExt,
    container, signal, virtual_list,
};

use super::super::{App, canvas};
use super::row::{self, Grid, Shown};
use super::rows::{ModelRow, action, list_rows};
use crate::catalog::Model;
use crate::endpoint::State;
use crate::recent::Seat;
use crate::tui::sigil_view::app::{Facet, Mark};
use crate::tui::sigil_view::ui::{alt_label, group, slotted};

pub(super) struct View {
    pub(super) choices: Rc<Vec<Seat>>,
    marks: Vec<Mark>,
    counts: [usize; 3],
    pub(super) groups: BTreeMap<String, State>,
    pub(super) facts: Rc<BTreeMap<Seat, Model>>,
    sections: bool,
    older: bool,
    aged: usize,
    pub(super) selected: usize,
}

impl View {
    pub(super) fn read(app: &App) -> Self {
        let picker = &app.picker;
        let sections = picker.draft.get().is_empty() && picker.facet.get() == Facet::All;
        let choices = picker.shown.get();
        let selected = picker.sel.get().min(choices.len().saturating_sub(1));
        Self {
            marks: choices.iter().map(|seat| app.mark(seat)).collect(),
            counts: picker.recent.get(),
            groups: picker.groups.get(),
            facts: picker.facts.get(),
            older: picker.older.get(),
            aged: if sections { picker.hidden.get() } else { 0 },
            sections,
            selected,
            choices,
        }
    }

    pub(super) fn rows(&self) -> Vec<ModelRow> {
        if self.sections {
            list_rows(&self.choices, self.counts, &self.groups, self.aged)
        } else {
            (0..self.choices.len()).map(ModelRow::Seat).collect()
        }
    }

    pub(super) fn seat(&self) -> Option<&Seat> {
        self.choices.get(self.selected)
    }

    fn seat_slots(&self, grid: Grid, index: usize) -> ListRowSlots {
        let seat = &self.choices[index];
        let state = self.groups.get(&seat.endpoint).copied();
        let listed: usize = self.counts.iter().sum();
        grid.row(
            seat,
            self.facts.get(seat),
            Shown {
                provider: !self.sections || index < listed || state.is_some_and(State::blocked),
                state,
            },
        )
    }

    fn fix_slots(&self, index: usize) -> ListRowSlots {
        let seat = &self.choices[index];
        let state = self
            .groups
            .get(&seat.endpoint)
            .copied()
            .unwrap_or(State::Error);
        let status = format!("{} · {}", state.word(), action(state));
        ListRowSlots {
            status: Some(status.into()),
            status_tone: Some(if state.failed() {
                Tone::Error
            } else {
                Tone::Neutral
            }),
            shrink: true,
            ..ListRowSlots::default()
        }
    }

    fn older_row(&self, app: &App, count: usize) -> NodeBuilder {
        let toggle = app.clone();
        let noun = if count == 1 { "model" } else { "models" };
        group(if self.older {
            format!("▾ hide {count} older {noun} · {}", alt_label('o'))
        } else {
            format!("▸ show {count} older {noun} · {}", alt_label('o'))
        })
        .pointer_down(move |event| {
            toggle.toggle_older_models();
            event.stop_propagation();
        })
    }
}

pub(super) fn list(
    app: &App,
    view: Rc<View>,
    grid: Grid,
    labels: Vec<String>,
    (rows, visible): (Vec<ModelRow>, usize),
) -> NodeBuilder {
    let selected = view.selected;
    let shown = rows
        .iter()
        .position(|row| matches!(row, ModelRow::Seat(at) | ModelRow::Fix(at) if *at == selected))
        .unwrap_or(0);
    let source = app.clone();
    let height = canvas::overlay_list_row() * visible as f32;
    container().layout(LayoutStyle::block().fill_width()).child(
        virtual_list(rows, move |_, row: &ModelRow| match row {
            ModelRow::Header(name) => row::section_heading(name),
            ModelRow::Provider(name, count) => {
                row::provider_heading(name, view.groups.get(name).copied(), *count)
            }
            ModelRow::Older(count) => view.older_row(&source, *count),
            ModelRow::Seat(index) => {
                let slots = view.seat_slots(grid, *index);
                let mark = view.marks[*index].glyph();
                let row = slotted(
                    *index == selected,
                    Some(mark),
                    labels[*index].clone(),
                    slots,
                );
                choosable(&source, row, *index)
            }
            ModelRow::Fix(index) => {
                let name = view.choices[*index].endpoint.clone();
                let row = slotted(*index == selected, Some(" "), name, view.fix_slots(*index));
                choosable(&source, row, *index)
            }
        })
        .row_height(canvas::overlay_list_row())
        .height(height)
        .viewport_height(height)
        .scroll_bind(app.picker.scroll)
        .follow_row(signal(shown)),
    )
}

fn choosable(app: &App, node: NodeBuilder, index: usize) -> NodeBuilder {
    let choose = app.clone();
    let follow = app.clone();
    node.pointer_move(move |_| {
        if !follow.auth.open.peek() && follow.picker.sel.peek() != index {
            follow.picker.sel.set(index);
        }
    })
    .pointer_down(move |event| {
        choose.picker.sel.set(index);
        choose.commit_model_edit();
        event.stop_propagation();
    })
}
