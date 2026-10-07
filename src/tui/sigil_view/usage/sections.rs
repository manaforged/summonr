use sigil::prelude::Paint;
use sigil::prelude::{
    AlignItems, FlexWrap, IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, Rgba8, Tone,
    container, heatmap, panel, progress, signal, sparkline,
};

use super::super::canvas::{self, blend, palette};
use super::report::{Metric, Report, hit, turn_secs};

use crate::provider::{compact, dollars};
use crate::session::Total;

const MODEL_ROWS: usize = 8;
const PLACE_CELLS: usize = 26;
const PLACE_NAME_CELLS: usize = 20;
const NAME_CELLS: usize = 24;
const NUMBER_CELLS: usize = 48;

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
                .gap(8.0)
                .align_items(AlignItems::Center),
        ),
        NodeBuilder::child,
    )
}

fn wrapped(value: impl Into<String>, color: Rgba8) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fill_width().min_size(0.0, 0.0))
        .child(
            sigil::kernel::text(value.into())
                .wrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .text_color(color),
        )
}

fn column(children: Vec<NodeBuilder>) -> NodeBuilder {
    children.into_iter().fold(
        container().layout(LayoutStyle::flex_column().gap(2.0)),
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
    let body = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(4.0))
        .child(strong(value))
        .child(visual.unwrap_or_else(|| colored(" ", palette.dim)))
        .child(wrapped(note, palette.dim));
    container()
        .layout(
            LayoutStyle::block()
                .grow(1.0)
                .shrink(1.0)
                .min_size(0.0, 0.0),
        )
        .child(panel().title(title).padding(16.0).child(body))
}

pub(super) fn cards(report: &Report) -> NodeBuilder {
    let total = &report.total;
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
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(12.0)
                .align_items(AlignItems::Stretch),
        )
        .child(card(
            "tokens",
            compact(total.input + total.output),
            Some(
                container()
                    .layout(LayoutStyle::block().fill_width().min_size(0.0, 0.0))
                    .child(trend),
            ),
            format!(
                "{} in · {} out",
                compact(total.input),
                compact(total.output)
            ),
        ))
        .child(card(
            "cache hit",
            format!("{:.1}%", report.cache_hit()),
            Some(
                container()
                    .layout(LayoutStyle::block().fill_width())
                    .child(cache),
            ),
            format!("{} tokens reused", compact(total.cached)),
        ))
        .child(card(
            "sessions",
            group(total.sessions),
            None,
            crate::text::plural(usize::try_from(total.turns).unwrap_or(usize::MAX), "turn")
                .replacen(&total.turns.to_string(), &group(total.turns), 1),
        ))
        .child(card("cost", cost, None, priced.to_string()))
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

fn name_cells(width: f32) -> usize {
    let cells = (width / canvas::cell().w).floor() as usize;
    cells.saturating_sub(NUMBER_CELLS).clamp(8, NAME_CELLS)
}

fn model_line(
    at: usize,
    name: &str,
    model: &Total,
    whole: f64,
    metric: Metric,
    name_cells: usize,
) -> NodeBuilder {
    let share = model.value(metric.cost()) / whole * 100.0;
    let tint = ink(at);
    let short = crate::text::ellipsize(name, name_cells);
    let short = format!(
        "{short}{}",
        " ".repeat(name_cells.saturating_sub(sigil::app::cell_width(&short)))
    );
    row(vec![
        colored("●", tint),
        colored(
            format!(
                "{short}{:>9}{:>8.0}%{:>9}{:>8.1}s{:>6.0}%",
                compact(model.input),
                hit(model),
                compact(model.output),
                turn_secs(model),
                share,
            ),
            tint,
        ),
    ])
}

pub(super) fn models(report: &Report, metric: Metric, width: f32) -> NodeBuilder {
    let whole = report
        .models
        .iter()
        .map(|(_, model)| model.value(metric.cost()))
        .sum::<f64>()
        .max(f64::MIN_POSITIVE);
    let name_cells = name_cells(width);
    let mut lines = vec![
        strong("Models"),
        row(vec![
            colored(" ", palette().code_comment),
            colored(
                format!(
                    "{:<width$}{:>9}{:>9}{:>9}{:>9}{:>7}",
                    "model",
                    "input",
                    "hit",
                    "output",
                    "turn",
                    "share",
                    width = name_cells,
                ),
                palette().code_comment,
            ),
        ]),
    ];
    lines.extend(
        report
            .models
            .iter()
            .take(MODEL_ROWS)
            .enumerate()
            .map(|(at, (name, model))| model_line(at, name, model, whole, metric, name_cells)),
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

pub(super) fn bottom(report: &Report, metric: Metric, width: f32) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .flex_wrap(FlexWrap::Wrap)
                .fill_width()
                .gap(24.0)
                .align_items(AlignItems::Start),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .shrink(0.0)
                        .gap(4.0)
                        .padding_right(8.0),
                )
                .child(models(report, metric, width)),
        )
        .child(
            container()
                .layout(LayoutStyle::flex_column().shrink(0.0).gap(16.0))
                .child(places(report, metric))
                .child(hours(report)),
        )
}

pub(super) fn spacer() -> impl IntoNode {
    container().layout(LayoutStyle::block().grow(1.0))
}
