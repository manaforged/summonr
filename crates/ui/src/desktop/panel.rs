use std::rc::Rc;

use sigil::prelude::{
    AlignItems, ControlSize, ListRowSlots, NodeBuilder, Overflow, Paint, Signal, Tone, Widget,
    button, container, derived, effect, interval, list_row,
};
use sigil::reactive::{on_cleanup, untrack};
use summonr_agent::hub::{Cmd, Open};
use summonr_agent::panel::{Action, Content, Fill, Item, Mood, Panel, Place, Plot, Series, Stat};

use super::style::{DANGER, L, M, S, SCROLLBAR, Text, XS, text};
use super::{App, Pane};

const CHART_HEIGHT: f32 = 240.0;
const TONES: [Tone; 4] = [Tone::Primary, Tone::Success, Tone::Warning, Tone::Error];

/// Keeps the tabs and started-agent prompt handoff in step with the workspace's panel files.
pub(super) fn install(app: &App) {
    drop(app.cmd.send(Cmd::ListPanels));
    let poll = app.clone();
    let timer = interval(summonr_agent::limits::limits().panel_poll(), move || {
        drop(poll.cmd.send(Cmd::ListPanels))
    });
    on_cleanup(move || timer.cancel());
    let waiting = app.window.panel_waiting;
    let watch = app.clone();
    effect(move || {
        let pane = super::split::current(&watch);
        if pane.model.with(|model| model.session.is_empty()) {
            return;
        }
        if let Some(prompt) = untrack(|| waiting.get()) {
            waiting.set(None);
            sigil::reactive::defer(move || pane.send_text(&prompt));
        }
    });
}

/// One tab per right-hand panel, after the built-in tabs.
pub(super) fn tabs(app: &App, pane: Pane) -> NodeBuilder {
    let app = app.clone();
    sigil::prelude::row()
        .align_items(sigil::prelude::AlignItems::Stretch)
        .gap(XS)
        .shrink(0.0)
        .children_reactive(move || {
            let shown = app.panels.shown.get();
            app.panels
                .list
                .get()
                .into_iter()
                .filter(|panel| panel.place == Place::Right)
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

/// The open right-hand panel. Its command runs only while this view is on screen.
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
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .fill_height()
        .min_width(0.0)
        .min_height(0.0)
        .children_reactive(move || {
            let Some(panel) = open.get() else {
                return vec![text(Text::Meta, "This panel is gone").into()];
            };
            keep_running(&runner, &panel);
            vec![body(&runner, panels)]
        })
}

/// The side panel the rail picked, while the workspace still has it.
pub(super) fn picked(app: &App) -> Signal<Option<Panel>> {
    let panels = app.panels;
    derived(move || {
        let name = panels.side.get()?;
        panels
            .list
            .get()
            .into_iter()
            .find(|panel| panel.place == Place::Side && panel.name == name)
    })
}

/// The picked side panel as the sidebar's body; it runs its command while it shows.
pub(super) fn side(app: &App, picked: Signal<Option<Panel>>) -> NodeBuilder {
    let app = app.clone();
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .children_reactive(move || {
            picked
                .get()
                .into_iter()
                .map(|panel| section(&app, panel))
                .collect::<Vec<_>>()
        })
}

fn keep_running(app: &App, panel: &Panel) {
    let (again, name) = (app.clone(), panel.name.clone());
    if panel.kind == Fill::Stream {
        drop(app.cmd.send(Cmd::OpenPanel(name.clone())));
        on_cleanup(move || drop(again.cmd.send(Cmd::ClosePanel(name))));
        return;
    }
    drop(app.cmd.send(Cmd::RunPanel(name.clone())));
    if let Some(every) = panel.every() {
        let timer = interval(every, move || {
            drop(again.cmd.send(Cmd::RunPanel(name.clone())))
        });
        on_cleanup(move || timer.cancel());
    }
}

fn body(app: &App, panels: crate::app::panels::Panels) -> NodeBuilder {
    let app = app.clone();
    sigil::prelude::column()
        .fill_width()
        .grow(1.0)
        .min_width(0.0)
        .min_height(0.0)
        .padding(L)
        .gap(M)
        .overflow_y(Overflow::Scroll)
        .children_reactive(move || {
            let name = untrack(|| panels.shown.get()).unwrap_or_default();
            render(&app, &name, panels.content.get())
        })
}

fn section(app: &App, panel: Panel) -> NodeBuilder {
    keep_running(app, &panel);
    let (app, results, name) = (app.clone(), app.panels.results, panel.name);
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .child(heading(&panel.title))
        .children_reactive(move || render(&app, &name, results.with(|all| all.get(&name).cloned())))
}

fn heading(title: &str) -> NodeBuilder {
    sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .align_items(sigil::prelude::AlignItems::Stretch)
        .fill_width()
        .padding_top(M)
        .padding_left(S)
        .padding_right(SCROLLBAR)
        .child(text(Text::Heading, title))
}

fn render(app: &App, name: &str, outcome: Option<Result<Content, String>>) -> Vec<NodeBuilder> {
    match outcome {
        None => Vec::new(),
        Some(Err(reason)) => vec![text(Text::Meta, reason).text_color(DANGER).into()],
        Some(Ok(Content::List { rows })) => list(app, rows),
        Some(Ok(Content::Surface(json))) => vec![surface(app, name, json)],
        Some(Ok(content)) => vec![draw(content)],
    }
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
        Content::List { .. } | Content::Surface(_) => text(Text::Meta, "No rows").into(),
    }
}

fn list(app: &App, rows: Vec<Item>) -> Vec<NodeBuilder> {
    rows.into_iter()
        .map(|item| {
            let run = (app.clone(), item.action.clone());
            list_row(
                false,
                item.title,
                ListRowSlots {
                    status: item.meta.map(Into::into),
                    status_tone: tone(item.tone),
                    activate: run
                        .1
                        .is_some()
                        .then(|| -> Rc<dyn Fn()> { Rc::new(move || act(&run.0, run.1.clone())) }),
                    ..ListRowSlots::default()
                },
            )
        })
        .collect()
}

fn surface(app: &App, panel: &str, json: String) -> NodeBuilder {
    let (app, panel) = (app.clone(), panel.to_owned());
    sigil::widgets::a2ui(move || json.clone())
        .activate(move |event, context| pressed(&app, &panel, event, context))
        .into()
}

fn pressed(app: &App, panel: &str, event: &str, context: &str) {
    let context: serde_json::Value = serde_json::from_str(context).unwrap_or_default();
    let ours = serde_json::from_value::<Action>(serde_json::json!({ event: context.clone() }));
    match ours {
        Ok(action) => act(app, Some(action)),
        Err(_) => drop(app.cmd.send(Cmd::ActPanel {
            name: panel.to_owned(),
            event: event.to_owned(),
            context,
        })),
    }
}

fn act(app: &App, action: Option<Action>) {
    match action {
        Some(Action::Start { prompt }) => {
            app.window.panel_waiting.set(Some(prompt));
            super::split::show(app, Open::New);
        }
        Some(Action::Open { session }) => super::split::show(app, Open::Resume(session)),
        None => {}
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
    plot.fill_width().height(CHART_HEIGHT).into()
}

fn stats(items: Vec<Stat>) -> NodeBuilder {
    items.into_iter().fold(
        sigil::prelude::column().fill_width().gap(S),
        |column, item| {
            let tile = sigil::widgets::stat(item.label, item.value);
            let tile = match tone(item.tone) {
                Some(tone) => tile.tone(tone),
                None => tile,
            };
            column.child(tile)
        },
    )
}

fn table(columns: Vec<String>, rows: Vec<Vec<String>>) -> NodeBuilder {
    let row = |cells: Vec<String>, role: Text| {
        cells.into_iter().fold(
            sigil::prelude::row()
                .fill_width()
                .gap(M)
                .align_items(AlignItems::Center),
            |row, cell| {
                row.child(
                    container()
                        .grow(1.0)
                        .width(0.0)
                        .min_width(0.0)
                        .min_height(0.0)
                        .child(text(role, cell).ellipsize()),
                )
            },
        )
    };
    rows.into_iter().fold(
        sigil::prelude::column()
            .fill_width()
            .gap(S)
            .child(row(columns, Text::Meta)),
        |table, cells| table.child(row(cells, Text::Body)),
    )
}

const fn tone(mood: Mood) -> Option<Tone> {
    match mood {
        Mood::Neutral => None,
        Mood::Success => Some(Tone::Success),
        Mood::Warning => Some(Tone::Warning),
        Mood::Danger => Some(Tone::Error),
    }
}
