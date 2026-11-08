use sigil::prelude::Space;
use sigil::prelude::{
    LayoutStyle, NodeBuilder, Signal, container, request_focus, scroll, segmented, signal, spacer,
};

use super::app::App;
use super::canvas::{self, palette};
use super::ui::{Dialog, Hints, Key, Size, Tab, tab_strip};
use summonr_agent::session::Spend;
use summonr_core::clock::now;

pub(super) mod report;
mod sections;
mod trend;

use report::{METRICS, Metric, Report};
use sections::{bottom, cards, colored};
use sigil::prelude::Widget;
use trend::trend;

/// The time ranges a usage report covers, by index.
pub(super) const RANGES: [&str; 4] = ["24h", "7d", "30d", "All"];
const CHROME_ROWS: u16 = 8;
const INSET: f32 = 24.0;

#[derive(Clone, Copy)]
pub(super) struct Panel {
    pub(super) open: Signal<bool>,
    pub(super) range: Signal<usize>,
    pub(super) metric: Signal<usize>,
    pub(super) local: Signal<bool>,
    pub(super) rows: Signal<Vec<Spend>>,
    pub(super) status: Signal<String>,
    pub(super) plans: Signal<Vec<summonr_agent::quota::Reading>>,
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
            plans: signal(Vec::new()),
        }
    }

    pub(super) fn start(self) {
        self.open.set(true);
        self.reading();
    }

    /// Marks the rows as being read again.
    pub(super) fn reading(self) {
        self.status.set("Reading sessions…".to_string());
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
        super::app::shift(&self.range, delta, RANGES.len());
    }

    pub(super) fn metric(self) {
        self.metric
            .set(super::ui::cycle(self.metric.peek(), 1, METRICS.len()));
    }

    pub(super) fn scope(self) {
        self.local.set(!self.local.peek());
    }
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    let viewport = app.viewport.peek();
    let width = Size::Page.width(viewport);
    let height = if canvas::is_window() {
        canvas::frame_body(viewport)
    } else {
        f32::from(viewport.1.saturating_sub(CHROME_ROWS).max(4)) * canvas::cell().h
    };
    let page = container()
        .focusable(true)
        .on_added(|_, id| request_focus(id))
        .layout(LayoutStyle::flex_column().fill_width().gap(Space::Lg))
        .children_reactive(move || page(&paint, width - INSET * 2.0));
    let hints = Hints::new()
        .key(Key::UpDown, "range")
        .key(Key::Tab, "tokens/cost")
        .key(Key::Fixed("←→"), "tabs")
        .key(Key::Enter, "this workspace/all")
        .key(Key::Esc, "close");
    Dialog::new(Size::Page, "Usage", hints)
        .header(Some(tab_strip(&app, Tab::Usage)))
        .fixed(viewport.1)
        .child(
            container()
                .layout(LayoutStyle::block().fill_width().height(height))
                .child(scroll(page).fill().follow_end(false)),
        )
        .build(app.usage.open, viewport, &app.model.peek().prefs.hotkeys)
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
    nodes.push(trend(&report, metric));
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
        .layout(LayoutStyle::flex_row().fill_width().gap(Space::Lg))
        .child(colored(scope, palette().dim))
        .child(spacer())
        .child(
            segmented(
                usage.range,
                RANGES.iter().map(ToString::to_string).collect(),
            )
            .height(canvas::overlay_list_row())
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
            .height(canvas::overlay_list_row())
            .label(format!("Metric: {}", metric.name())),
        )
}
