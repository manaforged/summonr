use std::collections::BTreeMap;

use sigil::prelude::{NodeBuilder, Rgba8, Signal, signal, text_field};

use super::app::App;
use super::canvas::{Cell, palette};
use super::chat::{accent, ellipsize, write_at};
use super::settings::{footer, frame};
use crate::bus::now;
use crate::provider::dollars;
use crate::session::{DAY, HOUR, Spend, Total, buckets};

const RANGES: [&str; 4] = ["24h", "7d", "30d", "lifetime"];
const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const AXIS: usize = 8;

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
    let input = text_field(signal(String::new())).class("w-full h-full");
    super::settings::layer(app, grid, input)
}

fn compact(value: f64) -> String {
    if value >= 1e9 {
        format!("{:.2}B", value / 1e9)
    } else if value >= 1e6 {
        format!("{:.1}M", value / 1e6)
    } else if value >= 1e3 {
        format!("{:.0}K", value / 1e3)
    } else {
        format!("{value:.0}")
    }
}

fn label(value: f64, cost: bool) -> String {
    if cost { dollars(value) } else { compact(value) }
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

fn grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    app.hits.clear();
    let panel = app.usage;
    let color = accent(&app.model.borrow().prefs.theme);
    if width < 40 || height < 12 {
        write_at(
            &mut cells,
            width,
            1,
            0,
            "usage · widen the window",
            palette().gray,
        );
        return cells;
    }
    let card = width.saturating_sub(2);
    let range = panel.range.get();
    let cost = panel.cost.get();
    let local = panel.local.get();
    let all = panel.rows.get();
    let status = panel.status.get();
    let rows: Vec<&Spend> = all.iter().filter(|row| !local || row.local).collect();
    let now = now();
    let room = card.saturating_sub(AXIS + 4);
    let first = rows.iter().map(|row| row.hour).min();
    let (span, count, first) = buckets(range, first, now, room);
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
    let palette_colors = colors();
    let tint = |model: &str| {
        order
            .iter()
            .position(|(name, _)| *name == model)
            .map_or(palette().dim, |index| {
                palette_colors[index % palette_colors.len()]
            })
    };

    let card_height = height;
    let table = order.len().max(1).min(card_height / 3);
    let chart = card_height.saturating_sub(table + 9).max(3);
    let left = 1;
    let top = 0;
    let bounds = (left, top, card, card_height);
    frame(&mut cells, width, bounds, color);
    let scope = if local {
        "this workspace"
    } else {
        "all workspaces"
    };
    write_at(
        &mut cells,
        width,
        left + 1,
        top,
        &format!(" ◷ usage · {scope} "),
        color,
    );
    let mut x = left + card.saturating_sub(3);
    for (index, name) in RANGES.iter().enumerate().rev() {
        let text = if index == range {
            format!("[{name}]")
        } else {
            format!(" {name} ")
        };
        x = x.saturating_sub(text.chars().count());
        write_at(
            &mut cells,
            width,
            x,
            top,
            &text,
            if index == range { color } else { palette().dim },
        );
    }
    footer(
        &mut cells,
        width,
        bounds,
        &[
            " ←→ range · tab tokens/cost · enter workspace · esc close ",
            " ←→ · tab · enter · esc ",
        ],
    );

    let inner = left + 2;
    let cached = if total.input == 0 {
        0.0
    } else {
        total.cached as f64 * 100.0 / total.input as f64
    };
    let summary = format!(
        "input {}   cached {cached:.1}%   output {}   cost {}",
        compact(total.input as f64),
        compact(total.output as f64),
        total.cost.map_or_else(|| "–".to_string(), dollars),
    );
    write_at(
        &mut cells,
        width,
        inner,
        top + 2,
        &ellipsize(&summary, card - 4),
        palette().text,
    );
    if !status.is_empty() || order.is_empty() {
        let note = if status.is_empty() {
            "no usage in this range"
        } else {
            &status
        };
        write_at(&mut cells, width, inner, top + 4, note, palette().dim);
        return cells;
    }

    let peak = columns
        .iter()
        .map(|column| column.values().map(|t| t.value(cost)).sum::<f64>())
        .fold(0.0_f64, f64::max)
        .max(f64::MIN_POSITIVE);
    let chart_top = top + 4;
    write_at(
        &mut cells,
        width,
        inner,
        chart_top,
        &format!("{:>7}", label(peak, cost)),
        palette().dim,
    );
    write_at(
        &mut cells,
        width,
        inner,
        chart_top + chart - 1,
        &format!("{:>7}", "0"),
        palette().dim,
    );
    let step = (room / count).clamp(1, 8);
    let bar = if step > 1 { step - 1 } else { 1 };
    for (index, column) in columns.iter().enumerate() {
        let mut stack: Vec<(f64, Rgba8)> = Vec::new();
        let mut sum = 0.0;
        for (name, _) in &order {
            if let Some(part) = column.get(name) {
                sum += part.value(cost) / peak * (chart * 8) as f64;
                stack.push((sum, tint(name)));
            }
        }
        let units = sum.round() as usize;
        let column_left = inner + AXIS + index * step;
        for level in 0..chart {
            let filled = units.saturating_sub(level * 8).min(8);
            let y = chart_top + chart - 1 - level;
            let (glyph, ink) = if filled == 0 {
                (if level == 0 { '·' } else { ' ' }, palette().dim)
            } else {
                let middle = (level * 8 + filled / 2) as f64;
                let ink = stack
                    .iter()
                    .find(|(edge, _)| middle < *edge)
                    .map_or(palette().dim, |(_, ink)| *ink);
                (BLOCKS[filled], ink)
            };
            let text: String = std::iter::repeat_n(glyph, bar).collect();
            write_at(&mut cells, width, column_left, y, &text, ink);
        }
    }
    let axis = chart_top + chart;
    write_at(&mut cells, width, inner + AXIS, axis, first, palette().dim);
    let unit = if span < DAY {
        format!("{}h", span / HOUR)
    } else {
        format!("{}d", span / DAY)
    };
    let right = format!(
        "1 column = {unit} · {}   now",
        if cost { "cost" } else { "tokens" }
    );
    let right_left = (inner + AXIS + count * step).saturating_sub(right.chars().count());
    write_at(
        &mut cells,
        width,
        right_left.max(inner + AXIS + first.len() + 2),
        axis,
        &right,
        palette().dim,
    );

    let head = axis + 2;
    let name = card.saturating_sub(4 + 2 + 36 + 2 + 12).clamp(16, 40);
    let bar_room = card.saturating_sub(49 + name);
    let share_left = inner + 2 + name + 36 + 2 + 5;
    write_at(
        &mut cells,
        width,
        inner,
        head,
        &ellipsize(
            &format!(
                "  {:<name$}{:>9}{:>8}{:>9}{:>10}   share",
                "model", "input", "cached", "output", "cost"
            ),
            card - 4,
        ),
        palette().gray,
    );
    let whole = order
        .iter()
        .map(|(_, t)| t.value(cost))
        .sum::<f64>()
        .max(f64::MIN_POSITIVE);
    for (index, (model_name, model)) in order.iter().take(table).enumerate() {
        let y = head + 1 + index;
        let ink = tint(model_name);
        write_at(&mut cells, width, inner, y, "█", ink);
        let cached = if model.input == 0 {
            0.0
        } else {
            model.cached as f64 * 100.0 / model.input as f64
        };
        let line = format!(
            "{:<name$}{:>9}{:>7.0}%{:>9}{:>10}",
            ellipsize(model_name, name - 1),
            compact(model.input as f64),
            cached,
            compact(model.output as f64),
            model.cost.map_or_else(|| "–".to_string(), dollars),
        );
        write_at(&mut cells, width, inner + 2, y, &line, palette().text);
        let share = model.value(cost) / whole;
        let length = ((share * bar_room as f64).round() as usize).max(1);
        let meter: String = std::iter::repeat_n('▬', length).collect();
        write_at(&mut cells, width, share_left, y, &meter, ink);
        write_at(
            &mut cells,
            width,
            share_left - 5,
            y,
            &format!("{:>3.0}%", share * 100.0),
            palette().dim,
        );
    }
    cells
}
