use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, Signal, WidgetLayoutExt, container, request_focus,
    scroll, segmented, signal,
};

use super::app::App;
use super::canvas::{self, palette};
use super::ui::{Dialog, Hints, Key, Size, Tab, tab_strip};
use crate::mail::now;
use crate::session::Spend;

mod report;
mod sections;
mod trend;

use report::{METRICS, Metric, Report};
use sections::{bottom, cards, colored, spacer};
use trend::trend;

const RANGES: [&str; 4] = ["24h", "7d", "30d", "all"];
const CHROME_ROWS: u16 = 8;
const INSET: f32 = 24.0;

#[derive(Clone, Copy)]
pub(super) struct Panel {
    pub(super) open: Signal<bool>,
    range: Signal<usize>,
    metric: Signal<usize>,
    local: Signal<bool>,
    rows: Signal<Vec<Spend>>,
    status: Signal<String>,
}

impl Panel {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            range: signal(2),
            metric: signal(0),
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
        self.metric.set((self.metric.peek() + 1) % METRICS.len());
    }

    pub(super) fn scope(self) {
        self.local.set(!self.local.peek());
    }
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    let (cols, rows) = app.viewport.peek();
    let width = Size::Page.width(cols);
    let height = f32::from(rows.saturating_sub(CHROME_ROWS).max(4)) * canvas::cell().h;
    let page = container()
        .focusable(true)
        .on_added(|_, id| request_focus(id))
        .layout(LayoutStyle::flex_column().fill_width().gap(16.0))
        .children_reactive(move || page(&paint, width - INSET * 2.0));
    let hints = Hints::new()
        .key(Key::UpDown, "range")
        .key(Key::Tab, "tokens/cost")
        .key(Key::Fixed("←→"), "tabs")
        .key(Key::Enter, "this workspace/all")
        .key(Key::Esc, "close");
    Dialog::new(Size::Page, "Usage", hints)
        .header(Some(tab_strip(&app, Tab::Usage)))
        .fixed(rows)
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().height(height))
                .child(scroll(page).fill().follow_end(false)),
        )
        .build(app.usage.open, cols, &app.model.peek().prefs.hotkeys)
}

fn page(app: &App, width: f32) -> Vec<NodeBuilder> {
    let usage = app.usage;
    let metric = METRICS[usage.metric.get().min(METRICS.len() - 1)];
    let local = usage.local.get();
    let range = usage.range.get();
    let all = usage.rows.get();
    let status = usage.status.get();
    let rows: Vec<&Spend> = all.iter().filter(|row| !local || row.local).collect();
    let mut nodes = vec![controls(usage, local, metric)];
    if !status.is_empty() || rows.is_empty() {
        let note = if !status.is_empty() {
            status.as_str()
        } else if local && !all.is_empty() {
            "no usage in this workspace yet; enter shows all workspaces"
        } else {
            "no usage yet"
        };
        nodes.push(colored(note, palette().dim));
        return nodes;
    }
    let report = Report::build(&rows, range, metric, now());
    if report.models.is_empty() {
        nodes.push(colored("no usage in this range", palette().dim));
        return nodes;
    }
    nodes.push(cards(&report));
    nodes.push(trend(&report, metric, width));
    nodes.push(bottom(&report, metric, width));
    nodes
}

fn controls(usage: Panel, local: bool, metric: Metric) -> NodeBuilder {
    let scope = if local {
        "this workspace"
    } else {
        "all workspaces"
    };
    container()
        .layout(LayoutStyle::flex_row().fill_width().gap(16.0))
        .child(colored(scope, palette().dim))
        .child(spacer())
        .child(
            segmented(
                usage.range,
                RANGES.iter().map(ToString::to_string).collect(),
            )
            .height(canvas::cell().h)
            .label("Range"),
        )
        .child(
            segmented(
                usage.metric,
                METRICS
                    .iter()
                    .map(|metric| metric.name().to_string())
                    .collect(),
            )
            .height(canvas::cell().h)
            .label(format!("Metric: {}", metric.name())),
        )
}
