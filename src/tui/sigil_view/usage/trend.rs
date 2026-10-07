use sigil::prelude::{
    AlignItems, JustifyContent, LayoutBuilderExt, LayoutStyle, NodeBuilder, Paint, Rgba8,
    container, panel,
};

use super::super::canvas::palette;
use super::report::{Metric, Report};
use super::sections::{colored, ink, strong};
use crate::session::{DAY, HOUR};

const TICKS: usize = 6;
const PLOT: f32 = 144.0;
const AXIS: f32 = 36.0;

pub(super) fn trend(report: &Report, metric: Metric, width: f32) -> NodeBuilder {
    let peak = report.columns.iter().copied().fold(0.0_f64, f64::max);
    let (divisor, unit) = scale(peak, metric);
    let colors: Vec<_> = (0..report.series.len()).map(ink).collect();
    let labels = labels(report);
    column(
        vec![
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .gap(8.0)
                        .align_items(AlignItems::Center),
                )
                .child(strong(format!(
                    "{} per {}",
                    title(metric),
                    period(report.span)
                )))
                .child(colored(
                    format!("· {unit} · since {}", report.first),
                    palette().dim,
                )),
            container()
                .layout(LayoutStyle::block().fill_width().width(width))
                .child(panel().padding(16.0).child(column(
                    vec![plot(report, peak, divisor, &colors), label_row(&labels)],
                    8.0,
                ))),
        ],
        8.0,
    )
}

fn plot(report: &Report, peak: f64, divisor: f64, colors: &[Rgba8]) -> NodeBuilder {
    let mut days = container().layout(
        LayoutStyle::flex_row()
            .grow(1.0)
            .fill_height()
            .min_size(0.0, 0.0)
            .gap(2.0)
            .align_items(AlignItems::End),
    );
    for index in 0..report.columns.len() {
        let parts: Vec<f64> = report
            .series
            .iter()
            .map(|series| series.get(index).copied().unwrap_or(0.0))
            .collect();
        days = days.child(day(&parts, peak, colors));
    }
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(PLOT)
                .gap(8.0)
                .align_items(AlignItems::Stretch),
        )
        .child(axis(peak, divisor))
        .child(days)
}

fn day(parts: &[f64], peak: f64, colors: &[Rgba8]) -> NodeBuilder {
    let sum: f64 = parts.iter().copied().sum();
    let mut column = container().layout(
        LayoutStyle::flex_column()
            .grow(1.0)
            .shrink(1.0)
            .min_size(0.0, 0.0)
            .fill_height(),
    );
    let open = (peak - sum).max(0.0);
    if open > 0.0 {
        column = column.child(grow(open as f32));
    }
    for (index, value) in parts.iter().enumerate().rev() {
        if *value <= 0.0 {
            continue;
        }
        let color = colors.get(index).copied().unwrap_or_else(|| ink(index));
        column = column.child(grow(*value as f32).bg(color));
    }
    if sum == 0.0 && open == 0.0 {
        column = column.child(grow(1.0));
    }
    column
}

fn axis(peak: f64, divisor: f64) -> NodeBuilder {
    let shown = if divisor == 0.0 { 0.0 } else { peak / divisor };
    let dim = palette().dim;
    container()
        .layout(
            LayoutStyle::flex_column()
                .width(AXIS)
                .shrink(0.0)
                .fill_height()
                .justify_content(JustifyContent::SpaceBetween),
        )
        .child(colored(tick_value(shown), dim))
        .child(colored(tick_value(shown / 2.0), dim))
        .child(colored("0", dim))
}

fn label_row(labels: &[String]) -> NodeBuilder {
    let marks = labels.iter().fold(
        container().layout(
            LayoutStyle::flex_row()
                .grow(1.0)
                .min_size(0.0, 0.0)
                .gap(2.0)
                .align_items(AlignItems::Start),
        ),
        |row, label| {
            row.child(
                container()
                    .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))
                    .child(colored(label.clone(), palette().code_comment)),
            )
        },
    );
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .align_items(AlignItems::Start),
        )
        .child(container().layout(LayoutStyle::block().width(AXIS).shrink(0.0)))
        .child(marks)
}

fn labels(report: &Report) -> Vec<String> {
    let count = report.columns.len();
    let every = count.div_ceil(TICKS).max(1);
    (0..count)
        .map(|at| {
            if at % every == 0 {
                tick(report.span, count - at)
            } else {
                String::new()
            }
        })
        .collect()
}

fn grow(value: f32) -> NodeBuilder {
    container().layout(
        LayoutStyle::block()
            .fill_width()
            .grow(value)
            .min_size(0.0, 0.0),
    )
}

fn column(children: Vec<NodeBuilder>, gap: f32) -> NodeBuilder {
    children.into_iter().fold(
        container().layout(LayoutStyle::flex_column().fill_width().gap(gap)),
        NodeBuilder::child,
    )
}

fn tick(span: u64, back: usize) -> String {
    let ago = span * back as u64;
    if ago < DAY {
        format!("{}h", ago / HOUR)
    } else {
        format!("{}d", ago / DAY)
    }
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

fn tick_value(value: f64) -> String {
    if value >= 100.0 || (value - value.round()).abs() < 0.05 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}
