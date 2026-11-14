use sigil::prelude::{AlignItems, NodeBuilder, Paint, Widget, interval};
use sigil::reactive::on_cleanup;
use summonr_agent::hub::Cmd;
use summonr_agent::quota::{Reading, Window};

use super::super::super::App;
use super::super::super::style::{DANGER, Text, XL, text};
use super::super::parts::{row, section};

/// Each plan's usage windows: saved ones on open while fresh, then a new read each cadence tick.
pub(super) fn view(app: &App) -> NodeBuilder {
    drop(app.cmd.send(Cmd::ReadPlans { due: false }));
    if let Some(every) = summonr_agent::quota::cadence() {
        let again = app.clone();
        let timer = interval(every, move || {
            drop(again.cmd.send(Cmd::ReadPlans { due: true }))
        });
        on_cleanup(move || timer.cancel());
    }
    let plans = app.usage.plans;
    sigil::prelude::column()
        .fill_width()
        .gap(XL)
        .align_items(AlignItems::Stretch)
        .children_reactive(move || plans.get().iter().map(plan).collect::<Vec<_>>())
}

fn plan(reading: &Reading) -> NodeBuilder {
    let title = match &reading.endpoint {
        Some(endpoint) => format!("{} ({endpoint})", reading.plan),
        None => reading.plan.clone(),
    };
    let mut rows: Vec<NodeBuilder> = reading.windows.iter().map(line).collect();
    if let Some(error) = &reading.error {
        rows.push(text(Text::Meta, error.clone()).text_color(DANGER).into());
    }
    if let Some(updated) = reading.updated() {
        rows.push(text(Text::Meta, updated).into());
    }
    section(Some(&title), rows)
}

fn line(window: &Window) -> NodeBuilder {
    let label = match &window.group {
        Some(group) => format!("{group} · {}", window.label()),
        None => window.label(),
    };
    let resets = window.resets_in();
    let used = text(Text::Label, format!("{:.0}% used", window.used)).nowrap();
    row(&label, resets.as_deref(), used.into())
}
