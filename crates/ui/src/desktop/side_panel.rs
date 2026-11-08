use std::cell::Cell;
use std::rc::Rc;

use sigil::prelude::{
    LayoutStyle, ListRowSlots, NodeBuilder, Paint, Signal, Tone, Widget, container, derived, effect,
    interval, list_row, signal,
};
use sigil::reactive::{on_cleanup, untrack};
use summonr_agent::hub::{Cmd, Open};
use summonr_agent::panel::{Action, Content, Item, Mood, Panel, Place};

use super::App;
use super::style::{DANGER, M, S, SCROLLBAR, Text, text};

thread_local! {
    static WAITING: Cell<Option<Signal<Option<String>>>> = const { Cell::new(None) };
}

/// Sends a started row's prompt to the agent that opens for it.
pub(super) fn install(app: &App) {
    let waiting = signal(None::<String>);
    WAITING.with(|slot| slot.set(Some(waiting)));
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

/// The sidebar sections of the panels placed there; each runs its command while it shows.
pub(super) fn view(app: &App) -> NodeBuilder {
    let panels = app.panels;
    let side = derived(move || {
        panels
            .list
            .get()
            .into_iter()
            .filter(|panel| panel.place == Place::Side)
            .collect::<Vec<Panel>>()
    });
    let app = app.clone();
    container()
        .layout(LayoutStyle::flex_column().fill_width())
        .children_reactive(move || {
            side.get()
                .into_iter()
                .map(|panel| section(&app, panel))
                .collect::<Vec<_>>()
        })
}

fn section(app: &App, panel: Panel) -> NodeBuilder {
    drop(app.cmd.send(Cmd::RunPanel(panel.name.clone())));
    if let Some(every) = panel.every() {
        let (again, name) = (app.clone(), panel.name.clone());
        let timer = interval(every, move || drop(again.cmd.send(Cmd::RunPanel(name.clone()))));
        on_cleanup(move || timer.cancel());
    }
    let (app, results, name) = (app.clone(), app.panels.results, panel.name);
    container()
        .layout(LayoutStyle::flex_column().fill_width())
        .child(heading(&panel.title))
        .children_reactive(move || match results.with(|all| all.get(&name).cloned()) {
            None => Vec::new(),
            Some(Err(reason)) => vec![text(Text::Meta, reason).text_color(DANGER).into()],
            Some(Ok(Content::List { rows })) => list(&app, rows),
            Some(Ok(content)) => vec![super::panel::draw(content)],
        })
}

fn heading(title: &str) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_top(M)
                .padding_left(S)
                .padding_right(SCROLLBAR),
        )
        .child(text(Text::Heading, title))
        .into()
}

/// One row per item; clicking a row runs its action.
pub(super) fn list(app: &App, rows: Vec<Item>) -> Vec<NodeBuilder> {
    rows.into_iter()
        .map(|item| {
            let run = (app.clone(), item.action.clone());
            list_row(
                false,
                item.title,
                ListRowSlots {
                    status: item.meta.map(Into::into),
                    status_tone: tone(item.tone),
                    activate: run.1.is_some().then(|| -> Rc<dyn Fn()> {
                        Rc::new(move || act(&run.0, run.1.clone()))
                    }),
                    ..ListRowSlots::default()
                },
            )
            .into()
        })
        .collect()
}

fn act(app: &App, action: Option<Action>) {
    match action {
        Some(Action::Start { prompt }) => {
            if let Some(waiting) = WAITING.with(Cell::get) {
                waiting.set(Some(prompt));
            }
            super::split::show(app, Open::New);
        }
        Some(Action::Open { session }) => super::split::show(app, Open::Resume(session)),
        None => {}
    }
}

const fn tone(mood: Mood) -> Option<Tone> {
    match mood {
        Mood::Neutral => None,
        Mood::Success => Some(Tone::Success),
        Mood::Warning => Some(Tone::Warn),
        Mood::Danger => Some(Tone::Error),
    }
}
