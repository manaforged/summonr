use sigil::prelude::{
    AlignItems, ControlSize, LayoutStyle, NodeBuilder, Overflow, Paint, Tone, Widget, button,
    container, derived, interval,
};
use sigil::reactive::on_cleanup;
use summonr_agent::hub::Cmd;
use summonr_agent::panel::{Content, Fill, Mood, Panel, Plot, Series, Stat};

use super::style::{DANGER, L, M, S, Text, XS, text};
use super::{App, Pane};

const CHART_HEIGHT: f32 = 240.0;
const TONES: [Tone; 4] = [Tone::Primary, Tone::Success, Tone::Warn, Tone::Error];

/// Keeps the tabs in step with the panel files of the chat's workspace.
pub(super) fn install(app: &App) {
    drop(app.cmd.send(Cmd::ListPanels));
    let poll = app.clone();
    let timer = interval(summonr_agent::limits::limits().panel_poll(), move || {
        drop(poll.cmd.send(Cmd::ListPanels))
    });
    on_cleanup(move || timer.cancel());
}

/// One tab per panel, after the built-in tabs.
pub(super) fn tabs(app: &App, pane: Pane) -> NodeBuilder {
    let app = app.clone();
    container()
        .layout(LayoutStyle::flex_row().gap(XS).shrink(0.0))
        .children_reactive(move || {
            let shown = app.panels.shown.get();
            app.panels
                .list
                .get()
                .into_iter()
                .map(|panel| {
                    let current =
                        pane == Pane::Custom && shown.as_deref() == Some(panel.name.as_str());
                    let open = app.clone();
                    let tab = button(panel.title.clone())
                        .size(ControlSize::Compact)
                        .activate(move || show(&open, &panel));
                    NodeBuilder::from(if current { tab.soft() } else { tab.ghost() })
                })
                .collect::<Vec<_>>()
        })
}

fn show(app: &App, panel: &Panel) {
    if panel.kind == Fill::Terminal && panel.problem.is_none() {
        let line = summonr_agent::panel::shell_line(panel, &app.workspace());
        app.open_terminal(super::terminal::NEW_TERMINAL, Some(&line));
        return;
    }
    let name = panel.name.as_str();
    if app.panels.shown.peek().as_deref() != Some(name) {
        app.panels.content.set(None);
        app.panels.shown.set(Some(name.to_string()));
    }
    if app.layout.peek().secondary != Some(Pane::Custom) {
        app.toggle_pane(Pane::Custom);
    }
}

/// The open panel. Its command runs only while this view is on screen.
pub(super) fn view(app: &App) -> NodeBuilder {
    let panels = app.panels;
    let open = derived(move || {
        let shown = panels.shown.get()?;
        panels
            .list
            .get()
            .into_iter()
            .find(|panel| panel.name == shown)
    });
    let runner = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .fill_height()
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            let Some(panel) = open.get() else {
                return vec![text(Text::Meta, "This panel is gone").into()];
            };
            drop(runner.cmd.send(Cmd::RunPanel(panel.name.clone())));
            if let Some(every) = panel.every() {
                let again = runner.clone();
                let name = panel.name;
                let timer = interval(every, move || {
                    drop(again.cmd.send(Cmd::RunPanel(name.clone())));
                });
                on_cleanup(move || timer.cancel());
            }
            vec![body(panels)]
        })
}

fn body(panels: crate::app::panels::Panels) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .grow(1.0)
                .min_size(0.0, 0.0)
                .padding(L)
                .gap(M)
                .overflow_y(Overflow::Scroll),
        )
        .children_reactive(move || match panels.content.get() {
            None => Vec::new(),
            Some(Err(reason)) => vec![text(Text::Meta, reason).text_color(DANGER).into()],
            Some(Ok(content)) => vec![draw(content)],
        })
}

fn draw(content: Content) -> NodeBuilder {
    match content {
        Content::Chart {
            kind,
            labels,
            series,
        } => chart(kind, labels, series),
        Content::Stat { items } => stats(items),
        Content::Table { columns, rows } => table(columns, rows),
        Content::Text { text: body } => text(Text::Meta, body).into(),
    }
}

fn chart(kind: Plot, labels: Vec<String>, series: Vec<Series>) -> NodeBuilder {
    let mut rows = series.into_iter().enumerate();
    let Some((_, first)) = rows.next() else {
        return text(Text::Meta, "No data").into();
    };
    let plot = sigil::widgets::chart(first.values).labels(labels).tooltip();
    let plot = rows.fold(plot, |plot, (index, row)| {
        plot.series(row.name, row.values, TONES[index % TONES.len()])
    });
    let plot = match kind {
        Plot::Line => plot.line(),
        Plot::Bar => plot,
        Plot::Area => plot.area(),
        Plot::Pie => plot.pie(),
    };
    plot.layout(LayoutStyle::block().fill_width().height(CHART_HEIGHT))
        .into()
}

fn stats(items: Vec<Stat>) -> NodeBuilder {
    items.into_iter().fold(
        container().layout(LayoutStyle::flex_column().fill_width().gap(S)),
        |column, item| {
            let tile = sigil::widgets::stat(item.label, item.value);
            let tile = match item.tone {
                Mood::Neutral => tile,
                Mood::Success => tile.tone(Tone::Success),
                Mood::Warning => tile.tone(Tone::Warn),
                Mood::Danger => tile.tone(Tone::Error),
            };
            column.child(tile)
        },
    )
}

fn table(columns: Vec<String>, rows: Vec<Vec<String>>) -> NodeBuilder {
    let row = |cells: Vec<String>, role: Text| {
        cells.into_iter().fold(
            container().layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .gap(M)
                    .align_items(AlignItems::Center),
            ),
            |row, cell| {
                row.child(
                    container()
                        .layout(LayoutStyle::block().grow(1.0).width(0.0).min_size(0.0, 0.0))
                        .child(text(role, cell).ellipsize()),
                )
            },
        )
    };
    rows.into_iter().fold(
        container()
            .layout(LayoutStyle::flex_column().fill_width().gap(S))
            .child(row(columns, Text::Meta)),
        |table, cells| table.child(row(cells, Text::Body)),
    )
}
