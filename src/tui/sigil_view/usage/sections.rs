use sigil::prelude::Paint;
use sigil::prelude::{
    AlignItems, IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, Rgba8, Tone, WidgetLayoutExt,
    chart, container, heatmap, panel, progress, signal, sparkline,
};

use super::super::canvas::{blend, palette};
use super::report::{Metric, Report, hit, turn_secs};

const SHOWN: usize = 5;
use crate::provider::{compact, dollars};
use crate::session::{DAY, HOUR, Total};

const TICKS: usize = 6;
const BAR_CELLS: usize = 16;
const MODEL_ROWS: usize = 8;
const PLACE_CELLS: usize = 26;
const PLACE_NAME_CELLS: usize = 20;
const NAME_CELLS: usize = 24;

fn inks() -> [Rgba8; 6] {
    let palette = palette();
    [
        palette.accent,
        palette.yellow,
        palette.green,
        palette.red,
        palette.code_keyword,
        palette.dim,
    ]
}

pub(super) fn ink(index: usize) -> Rgba8 {
    let inks = inks();
    inks[index.min(inks.len() - 1)]
}

pub(super) fn colored(value: impl Into<String>, color: Rgba8) -> NodeBuilder {
    container().layout(LayoutStyle::block().shrink(0.0)).child(
        sigil::kernel::text(value.into())
            .nowrap()
            .text_color(sigil::prelude::Token::OnSurface)
            .text_color(color),
    )
}

pub(super) fn strong(value: impl Into<String>) -> NodeBuilder {
    container().child(
        sigil::kernel::text(value.into())
            .nowrap()
            .text_color(sigil::prelude::Token::OnSurface)
            .font_weight(700)
            .text_color(palette().text),
    )
}

fn row(children: Vec<NodeBuilder>) -> NodeBuilder {
    children.into_iter().fold(
        container().layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .align_items(AlignItems::Center)
                .min_size(0.0, 0.0),
        ),
        NodeBuilder::child,
    )
}

fn column(children: Vec<NodeBuilder>) -> NodeBuilder {
    children.into_iter().fold(
        container().layout(LayoutStyle::flex_column().fill_width().gap(0.0)),
        NodeBuilder::child,
    )
}

pub(super) fn amount(value: f64, metric: Metric) -> String {
    if metric.cost() {
        dollars(value)
    } else {
        compact(value as u64)
    }
}

fn card(title: &str, value: String, visual: Option<NodeBuilder>, note: String) -> NodeBuilder {
    let palette = palette();
    let mut body = vec![strong(value)];
    body.push(visual.unwrap_or_else(|| colored(" ", palette.dim)));
    body.push(colored(note, palette.dim));
    container()
        .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))
        .child(panel().title(title).padding(8.0).child(column(body)))
}

pub(super) fn cards(report: &Report) -> NodeBuilder {
    let total = &report.total;
    let palette = palette();
    let trend = sparkline(
        report
            .columns
            .iter()
            .map(|value| *value as f32)
            .collect::<Vec<f32>>(),
    )
    .tone(Tone::Primary)
    .rows(1);
    let cache = progress(signal(report.cache_hit() as f32), 100.0).tone(Tone::Success);
    let cost = total.cost.map_or_else(|| "–".to_string(), dollars);
    let priced = if total.cost.is_some() {
        "priced models only"
    } else {
        "no prices for these models"
    };
    row(vec![
        card(
            "tokens",
            compact(total.input + total.output),
            Some(container().child(trend)),
            format!(
                "{} in · {} out",
                compact(total.input),
                compact(total.output)
            ),
        ),
        card(
            "cache hit",
            format!("{:.1}%", report.cache_hit()),
            Some(
                container()
                    .layout(LayoutStyle::block().fill_width())
                    .child(cache),
            ),
            format!("{} tokens reused", compact(total.cached)),
        ),
        card(
            "sessions",
            group(total.sessions),
            None,
            crate::text::plural(usize::try_from(total.turns).unwrap_or(usize::MAX), "turn")
                .replacen(&total.turns.to_string(), &group(total.turns), 1),
        ),
        card("cost", cost, None, priced.to_string()),
    ])
    .child(colored("", palette.dim))
}

fn group(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn tick(span: u64, back: usize) -> String {
    let ago = span * back as u64;
    if ago < DAY {
        format!("{}h", ago / HOUR)
    } else {
        format!("{}d", ago / DAY)
    }
}

pub(super) fn trend(report: &Report, metric: Metric, width: f32) -> NodeBuilder {
    let count = report.columns.len();
    let every = count.div_ceil(TICKS).max(1);
    let peak = report.columns.iter().copied().fold(0.0_f64, f64::max);
    let (scale, unit) = scale(peak, metric);
    let plot = chart(
        report
            .columns
            .iter()
            .map(|value| (value / scale) as f32)
            .collect::<Vec<f32>>(),
    )
    .tone(Tone::Primary)
    .bar()
    .labels((0..count).map(|at| {
        if at % every == 0 {
            tick(report.span, count - at)
        } else {
            String::new()
        }
    }))
    .height(220.0)
    .width(width);
    column(vec![
        row(vec![
            strong(format!("{} per {}", title(metric), period(report.span))),
            colored(format!("· {unit} · since {}", report.first), palette().dim),
        ]),
        container().child(plot),
    ])
}

fn period(span: u64) -> String {
    match span {
        HOUR => "hour".to_string(),
        DAY => "day".to_string(),
        span if span % DAY == 0 => format!("{} days", span / DAY),
        span => format!("{} hours", span / HOUR),
    }
}

fn title(metric: Metric) -> &'static str {
    match metric {
        Metric::Tokens => "Tokens",
        Metric::Cost => "Cost",
    }
}

fn scale(peak: f64, metric: Metric) -> (f64, &'static str) {
    if metric.cost() {
        (1.0, "dollars")
    } else if peak >= 1e9 {
        (1e9, "billions")
    } else if peak >= 1e6 {
        (1e6, "millions")
    } else if peak >= 1e3 {
        (1e3, "thousands")
    } else {
        (1.0, "tokens")
    }
}

fn bar(share: f64, cells: usize) -> String {
    "█".repeat(((share * cells as f64).round() as usize).max(1))
}

fn model_line(at: usize, name: &str, model: &Total, whole: f64, metric: Metric) -> NodeBuilder {
    let share = model.value(metric.cost()) / whole;
    let tint = ink(at.min(SHOWN));
    let short = crate::text::ellipsize(name, NAME_CELLS - 1);
    let short = format!(
        "{short}{}",
        " ".repeat(NAME_CELLS.saturating_sub(sigil::app::cell_width(&short)))
    );
    row(vec![
        colored("●", tint),
        colored(
            format!(
                "{short}{:>9}{:>8.0}%{:>9}{:>8.1}s",
                compact(model.input),
                hit(model),
                compact(model.output),
                turn_secs(model),
            ),
            palette().text,
        ),
        colored(bar(share, BAR_CELLS), tint),
        colored(format!("{:.0}%", share * 100.0), palette().dim),
    ])
}

pub(super) fn models(report: &Report, metric: Metric) -> NodeBuilder {
    let whole = report
        .models
        .iter()
        .map(|(_, model)| model.value(metric.cost()))
        .sum::<f64>()
        .max(f64::MIN_POSITIVE);
    let mut lines = vec![
        strong("Models"),
        colored(
            format!(
                "  {:<width$}{:>9}{:>9}{:>9}{:>9}  share",
                "model",
                "input",
                "hit",
                "output",
                "turn",
                width = NAME_CELLS,
            ),
            palette().code_comment,
        ),
    ];
    lines.extend(
        report
            .models
            .iter()
            .take(MODEL_ROWS)
            .enumerate()
            .map(|(at, (name, model))| model_line(at, name, model, whole, metric)),
    );
    column(lines)
}

pub(super) fn places(report: &Report, metric: Metric) -> NodeBuilder {
    let peak = report
        .places
        .first()
        .map_or(1.0, |(_, value)| value.max(f64::MIN_POSITIVE));
    let mut lines = vec![strong("Workspaces")];
    for (name, value) in &report.places {
        let filled = ((value / peak) * PLACE_CELLS as f64).round() as usize;
        let short = crate::text::ellipsize(name, PLACE_NAME_CELLS);
        let pad = PLACE_NAME_CELLS.saturating_sub(sigil::app::cell_width(&short));
        lines.push(row(vec![
            colored(format!("{short}{}", " ".repeat(pad)), palette().text),
            colored("█".repeat(filled.max(1)), palette().accent),
            colored(amount(*value, metric), palette().dim),
        ]));
    }
    column(lines)
}

pub(super) fn hours(report: &Report) -> NodeBuilder {
    let peak = report
        .hours
        .iter()
        .copied()
        .fold(0.0_f64, f64::max)
        .max(f64::MIN_POSITIVE);
    let palette = palette();
    let hours = report.hours;
    let strip = heatmap(24, 1, move |x, _| {
        blend(palette.code_bg, palette.accent, (hours[x] / peak) as f32)
    });
    column(vec![
        row(vec![
            strong("By hour"),
            colored("UTC", palette.code_comment),
        ]),
        container().child(strip),
        colored("0      6      12     18    23", palette.code_comment),
    ])
}

pub(super) fn bottom(report: &Report, metric: Metric) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(32.0)
                .align_items(AlignItems::Start),
        )
        .child(
            container()
                .layout(LayoutStyle::block().grow(3.0).min_size(0.0, 0.0))
                .child(models(report, metric)),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .grow(2.0)
                        .gap(16.0)
                        .min_size(0.0, 0.0),
                )
                .child(places(report, metric))
                .child(hours(report)),
        )
}

pub(super) fn spacer() -> impl IntoNode {
    container().layout(LayoutStyle::block().grow(1.0))
}
