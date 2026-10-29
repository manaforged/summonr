use sigil::prelude::{
    NodeBuilder, Signal, WidgetLayoutExt, derived, signal, text_field, virtual_list,
};

use super::app::App;
use super::canvas;
use super::ui::{Dialog, Hints, Key, List, Size, pick};
use crate::tui::matching::all_words;

const ROW_MARKER: usize = 2;
const DIALOG_PADDING: f32 = 16.0;

#[derive(Clone, Copy)]
pub(super) struct View {
    pub(super) open: Signal<bool>,
    list: List,
    focus: Signal<u64>,
}

impl View {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            list: List::new(),
            focus: signal(0),
        }
    }

    pub(super) fn show(self) {
        self.list.query.set(String::new());
        self.list.scroll.set(0.0);
        self.focus.set(self.focus.peek() + 1);
        self.open.set(true);
    }
}

pub(super) fn shift(app: &App, delta: isize) {
    let total = lines(app, &app.help.list.query.peek()).len();
    let row = canvas::overlay_list_row();
    let max = total.saturating_sub(visible(app)) as f32 * row;
    let next = delta as f32 * row + app.help.list.scroll.peek();
    app.help.list.scroll.set(next.clamp(0.0, max));
}

fn visible(app: &App) -> usize {
    canvas::overlay_shown(usize::MAX, app.viewport.peek().1).saturating_sub(1)
}

fn lines(app: &App, query: &str) -> Vec<String> {
    let width = row_cells(app.viewport.peek().0);
    crate::tui::help_text(&app.model.get().prefs.hotkeys)
        .lines()
        .filter(|line| all_words(line, query))
        .map(|line| crate::display::ellipsize(line, width))
        .collect()
}

fn row_cells(cols: u16) -> usize {
    let cell = canvas::cell().w;
    let dialog = Size::Normal.width(cols) / cell;
    let padding = (DIALOG_PADDING / cell).ceil();
    ((dialog - padding).max(0.0) as usize).saturating_sub(ROW_MARKER)
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let View { list, focus, .. } = app.help;
    let List { query, scroll, .. } = list;
    let keys = app.clone();
    let cols = app.viewport.peek().0;
    let visible = visible(&app) as f32;
    let rows = derived(move || lines(&keys, &query.get()));
    let hints = Hints::new()
        .key(Key::Fixed("type"), "to filter")
        .key(Key::Fixed("scroll"), "for more")
        .key(Key::Esc, "close");
    Dialog::new(Size::Reading, "Help", hints)
        .child(
            text_field(query)
                .placeholder("find")
                .focus_when(focus)
                .autofocus()
                .change(move |_| scroll.set(0.0))
                .height(canvas::overlay_field()),
        )
        .child(
            virtual_list(rows, move |_, line: &String| {
                pick(false, line.clone(), String::new())
            })
            .row_height(canvas::overlay_list_row())
            .height(canvas::overlay_list_row() * visible)
            .viewport_height(canvas::overlay_list_row() * visible)
            .scroll_bind(scroll),
        )
        .build(app.help.open, cols, &app.model.peek().prefs.hotkeys)
}
