use std::collections::BTreeMap;

use sigil::prelude::ColorAtom;
use sigil::prelude::IntoNode;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::NodeBuilder;
use sigil::prelude::NodeStyleExt;
use sigil::prelude::Rgba8;
use sigil::prelude::Signal;
use sigil::prelude::Tone;
use sigil::prelude::chart;
use sigil::prelude::container;
use sigil::prelude::panel;
use sigil::prelude::request_focus;
use sigil::prelude::scroll;
use sigil::prelude::signal;
use sigil::style::Style;

use super::app::App;
use super::canvas::palette;
use super::prelude::*;
use crate::mail::now;
use crate::provider::{compact, dollars};
use crate::session::{DAY, HOUR, Spend, Total, buckets};

const RANGES: [&str; 4] = ["24h", "7d", "30d", "lifetime"];
const ROOM: usize = 78;

#[derive(Clone, Copy)]
pub(super) struct Panel {
    pub(super) open: Signal<bool>,
    range: Signal<usize>,
    cost: Signal<bool>,
    local: Signal<bool>,
    rows: Signal<Vec<Spend>>,
    status: Signal<String>,
}

impl Panel {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            range: signal(2),
            cost: signal(false),
            local: signal(false),
            rows: signal(Vec::new()),
            status: signal(String::new()),
        }
    }

    pub(super) fn start(self) {
        self.open.set(true);
        self.status.set("reading sessions…".to_string());
    }

    pub(super) fn update(self, result: Result<Vec<Spend>, String>) {
        match result {
            Ok(rows) => {
                self.status.set(String::new());
                self.rows.set(rows);
            }
            Err(error) => self.status.set(error),
        }
    }

    pub(super) fn shift(self, delta: isize) {
        let next = self.range.peek().saturating_add_signed(delta);
        self.range.set(next.min(RANGES.len() - 1));
    }

    pub(super) fn metric(self) {
        self.cost.set(!self.cost.peek());
    }

    pub(super) fn scope(self) {
        self.local.set(!self.local.peek());
    }
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    shell(
        app.usage.open,
        "Usage",
        app.viewport.peek().0,
        container()
            .layout(LayoutStyle::flex_column().fill_width())
            .children_reactive(move || vec![body(&paint)]),
    )
}

fn label(value: f64, cost: bool) -> String {
    if cost {
        dollars(value)
    } else {
        compact(value as u64)
    }
}

fn colors() -> [Rgba8; 6] {
    let palette = palette();
    [
        palette.accent,
        palette.yellow,
        palette.green,
        palette.red,
        palette.code_keyword,
        palette.cyan,
    ]
}

fn tone(index: usize) -> Tone {
    const TONES: [Tone; 6] = [
        Tone::Primary,
        Tone::Warn,
        Tone::Success,
        Tone::Error,
        Tone::Info,
        Tone::Neutral,
    ];
    TONES[index % TONES.len()]
}

fn tinted(value: String, index: usize) -> impl IntoNode + 'static {
    let colors = colors();
    let ink = colors[index % colors.len()];
    sigil::kernel::text(value)
        .nowrap()
        .class(sigil::style::Token::OnSurface.text())
        .style(Style::new().text_color(ColorAtom::Rgba8(ink)))
}

fn scroll_panel(title: String, content: NodeBuilder) -> NodeBuilder {
    let content = container()
        .focusable(true)
        .on_added(|_, id| request_focus(id))
        .child(panel().title(title).child(content));
    container()
        .layout(LayoutStyle::block().fill())
        .child(scroll(content).fill().auto_pin_to_bottom(false))
}

const SHOWN: usize = 5;
const TICKS: usize = 6;
const OTHER: &str = "other";

fn axis(peak: f64, cost: bool) -> (f64, &'static str) {
    if cost {
        (1.0, "")
    } else if peak >= 1e9 {
        (1e9, " · billions")
    } else if peak >= 1e6 {
        (1e6, " · millions")
    } else if peak >= 1e3 {
        (1e3, " · thousands")
    } else {
        (1.0, "")
    }
}

fn bucket(span: u64, back: usize) -> String {
    let ago = span * back as u64;
    if ago < DAY {
        format!("{}h", ago / HOUR)
    } else {
        format!("{}d", ago / DAY)
    }
}

fn body(app: &App) -> NodeBuilder {
    let usage = app.usage;
    let range = usage.range.get();
    let cost = usage.cost.get();
    let local = usage.local.get();
    let all = usage.rows.get();
    let status = usage.status.get();
    let rows: Vec<&Spend> = all.iter().filter(|row| !local || row.local).collect();
    let now = now();
    let first = rows.iter().map(|row| row.hour).min();
    let (span, count, first) = buckets(range, first, now, ROOM);
    let start = now.saturating_sub(span * count as u64);

    let mut total = Total::default();
    let mut models: BTreeMap<&str, Total> = BTreeMap::new();
    let mut columns: Vec<BTreeMap<&str, Total>> = (0..count).map(|_| BTreeMap::new()).collect();
    for row in rows.iter().filter(|row| row.hour + HOUR > start) {
        total.add(row);
        models.entry(&row.model).or_default().add(row);
        let index = (row.hour.saturating_sub(start) / span) as usize;
        columns[index.min(count - 1)]
            .entry(&row.model)
            .or_default()
            .add(row);
    }
    let mut order: Vec<(&str, Total)> = models.into_iter().collect();
    order.sort_by(|a, b| b.1.value(cost).total_cmp(&a.1.value(cost)));

    let scope = if local {
        "this workspace"
    } else {
        "all workspaces"
    };
    let title = format!(" ◷ usage · {scope} ");
    let mut content = container().layout(LayoutStyle::flex_column().gap(6.0));
    if !status.is_empty() || order.is_empty() {
        let note = if status.is_empty() {
            "no usage in this range"
        } else {
            status.as_str()
        };
        return scroll_panel(
            title,
            content.child(
                sigil::kernel::text(note.to_string())
                    .nowrap()
                    .class(sigil::style::Token::OnSurface.text())
                    .style(Style::new().text_color(ColorAtom::Rgba8(palette().dim))),
            ),
        );
    }

    let peak = columns
        .iter()
        .map(|column| column.values().map(|total| total.value(cost)).sum::<f64>())
        .fold(0.0_f64, f64::max)
        .max(f64::MIN_POSITIVE);
    let (scale, suffix) = axis(peak, cost);
    let named: Vec<&str> = order.iter().take(SHOWN).map(|(name, _)| *name).collect();
    let bars = |want: Option<&str>| {
        columns
            .iter()
            .map(|column| {
                column
                    .iter()
                    .filter(|(name, _)| match want {
                        Some(want) => **name == want,
                        None => !named.contains(name),
                    })
                    .map(|(_, total)| total.value(cost))
                    .sum::<f64>()
                    / scale
            })
            .map(|value| value as f32)
            .collect::<Vec<_>>()
    };
    let every = count.div_ceil(TICKS).max(1);
    let mut plot = chart(std::iter::empty())
        .bar()
        .labels((0..count).map(|index| {
            if index % every == 0 {
                bucket(span, count - index)
            } else {
                String::new()
            }
        }))
        .height(240.0);
    for (index, name) in named.iter().enumerate() {
        plot = plot.series(*name, bars(Some(name)), tone(index));
    }
    if order.len() > SHOWN {
        plot = plot.series(OTHER, bars(None), tone(SHOWN));
    }
    content = content.child(plot);
    let unit = if span < DAY {
        format!("{}h", span / HOUR)
    } else {
        format!("{}d", span / DAY)
    };
    content = content.child(
        sigil::kernel::text(format!(
            "one column = {unit} · {}{suffix} · oldest {first} · peak {}",
            if cost { "cost" } else { "tokens" },
            label(peak, cost),
        ))
        .nowrap()
        .class(sigil::style::Token::OnSurface.text())
        .style(Style::new().text_color(ColorAtom::Rgba8(palette().dim))),
    );
    let cached = if total.input == 0 {
        0.0
    } else {
        total.cached as f64 * 100.0 / total.input as f64
    };
    content = content.child(
        sigil::kernel::text(format!(
            "total   input {}   cached {cached:.1}%   output {}   cost {}",
            compact(total.input),
            compact(total.output),
            total.cost.map_or_else(|| "–".to_string(), dollars),
        ))
        .nowrap()
        .class(sigil::style::Token::OnSurface.text())
        .style(Style::new().text_color(ColorAtom::Rgba8(palette().dim))),
    );
    let whole = order
        .iter()
        .map(|(_, total)| total.value(cost))
        .sum::<f64>()
        .max(f64::MIN_POSITIVE);
    for (index, (name, model)) in order.iter().enumerate() {
        let cached = if model.input == 0 {
            0.0
        } else {
            model.cached as f64 * 100.0 / model.input as f64
        };
        content = content.child(tinted(
            format!(
                "{name}   in {}   cached {cached:.1}%   out {}   {}   {:.1}%",
                compact(model.input),
                compact(model.output),
                model.cost.map_or_else(|| "–".to_string(), dollars),
                model.value(cost) / whole * 100.0,
            ),
            index,
        ));
    }
    scroll_panel(title, content)
}
