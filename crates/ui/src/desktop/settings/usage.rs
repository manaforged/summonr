use sigil::prelude::{
    AlignItems, JustifyContent, NodeBuilder, Paint, Radius, Signal, Space, Widget, container,
    effect, segmented, signal,
};
use sigil::reactive::untrack;
use summonr_agent::provider::{compact, dollars};
use summonr_agent::session::{Spend, Total};

use super::super::App;
use super::super::style::{CHART, CHART_BAR, L, M, S, Text, XL, XS, text};
use super::parts::{row, section};
use crate::usage::RANGES;
use crate::usage::report::{METRICS, Metric, Report, hit};

mod plans;

const SCOPES: [&str; 2] = ["Everywhere", "This repository"];
const NONE: &str = "No usage yet";
const NOTHING_HERE: &str = "No usage in this range";

/// Spend over a range: totals, a chart over time, and the split by model and repository.
pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    app.load_usage();
    let usage = app.usage;
    vec![
        plans::view(app),
        controls(usage.range, usage.local, usage.metric),
        sigil::prelude::column()
            .fill_width()
            .gap(XL)
            .align_items(AlignItems::Stretch)
            .children_reactive(move || {
                let rows = usage.rows.get();
                let status = usage.status.get();
                let local = usage.local.get();
                let metric = METRICS[usage.metric.get().min(METRICS.len() - 1)];
                let range = usage.range.get();
                report(&rows, &status, local, range, metric)
            }),
    ]
}

fn controls(range: Signal<usize>, local: Signal<bool>, metric: Signal<usize>) -> NodeBuilder {
    let scope = signal(usize::from(local.peek()));
    effect(move || {
        let here = scope.get() == 1;
        if untrack(|| local.get()) != here {
            local.set(here);
        }
    });
    let labels = |options: &[&str]| options.iter().map(|option| (*option).to_owned()).collect();
    let metrics = METRICS
        .iter()
        .map(|metric| title(*metric).to_owned())
        .collect();
    sigil::prelude::row()
        .fill_width()
        .gap(M)
        .align_items(AlignItems::Center)
        .child(segmented(range, labels(&RANGES)).label("Range"))
        .child(segmented(scope, labels(&SCOPES)).label("Where"))
        .child(container().grow(1.0))
        .child(segmented(metric, metrics).label("Chart"))
}

fn report(
    rows: &[Spend],
    status: &str,
    local: bool,
    range: usize,
    metric: Metric,
) -> Vec<NodeBuilder> {
    if !status.is_empty() && rows.is_empty() {
        return vec![text(Text::Meta, status.to_owned()).into()];
    }
    let shown: Vec<&Spend> = rows.iter().filter(|row| !local || row.local).collect();
    if shown.is_empty() {
        return vec![text(Text::Meta, NONE).into()];
    }
    let report = Report::build(&shown, range, metric, summonr_core::clock::now());
    if report.models.is_empty() {
        return vec![text(Text::Meta, NOTHING_HERE).into()];
    }
    let models = report
        .models
        .iter()
        .map(|(name, total)| row(name, None, line(total)))
        .collect();
    let places = report
        .places
        .iter()
        .map(|(name, value)| row(name, None, amount(*value, metric)))
        .collect();
    let mut sections = vec![
        totals(&report.total),
        section(Some(per(metric)), vec![chart(&report)]),
        section(Some("Models"), models),
        section(Some("Repositories"), places),
    ];
    if !report.sets.is_empty() {
        let sets = report
            .sets
            .iter()
            .map(|(name, total)| row(name, None, line(total)))
            .collect();
        sections.push(section(Some("Skill sets"), sets));
    }
    sections
}

fn totals(total: &Total) -> NodeBuilder {
    let cost = total.cost.map_or_else(|| "—".to_owned(), dollars);
    let tiles = [
        ("Cost", cost),
        ("Tokens", compact(total.input + total.output)),
        ("Turns", total.turns.to_string()),
        ("Cache hit", format!("{:.0}%", hit(total))),
    ];
    let mut strip = sigil::prelude::row()
        .align_items(sigil::prelude::AlignItems::Stretch)
        .fill_width()
        .padding_x(L)
        .padding_y(L)
        .gap(L);
    for (label, value) in tiles {
        strip = strip.child(
            sigil::prelude::column()
                .grow(1.0)
                .flex_basis(0.0)
                .min_width(0.0)
                .min_height(0.0)
                .gap(XS)
                .child(text(Text::Meta, label))
                .child(text(Text::Title, value)),
        );
    }
    section(None, vec![strip])
}

fn chart(report: &Report) -> NodeBuilder {
    let peak = report.columns.iter().copied().fold(0.0_f64, f64::max);
    let mut bars = sigil::prelude::row()
        .fill_width()
        .height(CHART)
        .gap(Space::Xxs)
        .align_items(AlignItems::End);
    for value in &report.columns {
        bars = bars.child(bar(*value, peak));
    }
    sigil::prelude::column()
        .fill_width()
        .padding_x(L)
        .padding_y(L)
        .gap(S)
        .child(bars)
        .child(
            sigil::prelude::row()
                .gap(sigil::prelude::Space::None)
                .align_items(sigil::prelude::AlignItems::Stretch)
                .fill_width()
                .justify_content(JustifyContent::SpaceBetween)
                .child(text(Text::Meta, report.first))
                .child(text(Text::Meta, "Now")),
        )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "a share of a chart's height fits an f32"
)]
fn bar(value: f64, peak: f64) -> NodeBuilder {
    let share = if peak > 0.0 {
        (value / peak) as f32
    } else {
        0.0
    };
    let height = if value > 0.0 {
        (share * CHART).max(2.0)
    } else {
        0.0
    };
    container()
        .grow(1.0)
        .flex_basis(0.0)
        .min_width(0.0)
        .min_height(0.0)
        .height(height)
        .background(CHART_BAR)
        .radius(Radius::Sm)
}

fn line(total: &Total) -> NodeBuilder {
    let tokens = compact(total.input + total.output);
    let words = match total.cost {
        Some(cost) => format!("{tokens} · {}", dollars(cost)),
        None => tokens,
    };
    text(Text::Label, words).nowrap().into()
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a token total summed as f64 is a whole, non-negative count"
)]
fn amount(value: f64, metric: Metric) -> NodeBuilder {
    let words = match metric {
        Metric::Cost => dollars(value),
        Metric::Tokens => compact(value as u64),
    };
    text(Text::Label, words).nowrap().into()
}

const fn title(metric: Metric) -> &'static str {
    match metric {
        Metric::Tokens => "Tokens",
        Metric::Cost => "Cost",
    }
}

const fn per(metric: Metric) -> &'static str {
    match metric {
        Metric::Tokens => "Tokens over time",
        Metric::Cost => "Cost over time",
    }
}
