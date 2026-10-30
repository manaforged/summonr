use sigil::prelude::{LayoutStyle, NodeBuilder, NodeEventsExt, signal, virtual_list};

use super::{App, canvas};
use crate::AuthInfo;
use crate::endpoint::State;
use crate::tui::sigil_view::app::{Login, Note, matching, section};
use crate::tui::sigil_view::ui::{
    Bindings, Dialog, Hints, Key, Size, Tab, alt, browser_columns, browser_layout, browser_split,
    group, overlay_list, pick, tab_strip,
};

mod detail;
mod form;

use form::{Input, field, form, notes};

const SPLIT_COLS: u16 = 110;
const LIST_CELLS: f32 = 58.0;
const LIST_CHROME: u16 = 5;
const NARROW_NOTE_ROWS: usize = 2;
const KEY_NOTE: &str = "A key is a secret from the provider's website that lets summonr use its models. Providers at the top need none.";

pub(in crate::tui::sigil_view) fn keys() -> Bindings {
    let mut bindings = alt("d", |app, event| {
        event.stop_propagation();
        app.confirm_logout();
    });
    bindings.extend(alt("k", |app, event| {
        event.stop_propagation();
        app.key_entry();
    }));
    bindings.extend(alt("r", |app, event| {
        event.stop_propagation();
        app.toggle_route(crate::endpoint::Toggle::Region);
    }));
    bindings.extend(alt("a", |app, event| {
        event.stop_propagation();
        app.toggle_route(crate::endpoint::Toggle::Access);
    }));
    bindings
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    let state = app.clone();
    let hotkeys = app.model.peek().prefs.hotkeys.clone();
    let (cols, rows) = app.viewport.peek();
    Dialog::new(Size::Page, "Providers", Hints::new())
        .header(Some(tab_strip(&app, Tab::Providers)))
        .fixed(canvas::picker_rows(rows))
        .live_hints(move || hints(&state))
        .child(super::dialog_body(move || body(&paint)))
        .build(app.auth.open, cols, &hotkeys)
}

fn body(app: &App) -> Vec<NodeBuilder> {
    let listing_app = app.clone();
    let listing = matches!(app.auth.step.get(), Login::List(_));
    let cols = app.viewport.get().0;
    if !listing && !browser_split(cols, SPLIT_COLS) {
        return form(app, Size::Page.cells(cols));
    }
    vec![
        field(app, Input::Search, listing),
        super::dialog_body(move || self::listing(&listing_app)),
    ]
}

pub(super) fn hints(app: &App) -> Hints {
    match app.auth.step.get() {
        Login::List(_) => listing_hints(app),
        Login::Key(_) => Hints::new()
            .key(Key::Enter, "save and check")
            .key(Key::Esc, "back")
            .right(summonr_core::text::plural(
                app.auth.key.get().chars().count(),
                "char",
            )),
        Login::Checking => Hints::new().key(Key::Esc, "cancel"),
        Login::SignIn { note, .. } if !note.is_empty() => Hints::new()
            .key(Key::Enter, "try again")
            .key(Key::Esc, "back"),
        Login::SignIn { asking: true, .. } => Hints::new()
            .key(Key::Enter, "send code")
            .key(Key::Esc, "cancel"),
        Login::SignIn { asking: false, .. } => Hints::new().key(Key::Esc, "cancel"),
        Login::Confirm => Hints::new().key(Key::Enter, "yes").key(Key::Esc, "no"),
    }
}

fn listing_hints(app: &App) -> Hints {
    let rows = matching(app.auth.choices.get(), &app.auth.list.query.get());
    let selected = app.auth.list.cursor.get().min(rows.len().saturating_sub(1));
    let Some(choice) = rows.get(selected) else {
        return Hints::new().key(Key::Esc, "close").right("0 / 0");
    };
    let stacked = app.picker.open.get();
    let mut hints = Hints::new();
    if !stacked {
        hints = hints.key(Key::UpDown, "move");
    }
    hints = hints.key(Key::Enter, choice.key_action());
    if choice.paste_key() {
        hints = hints.key(Key::Alt('k'), "paste a key");
    }
    if let Some(region) = choice.switch(crate::endpoint::Toggle::Region) {
        hints = hints.key(Key::Alt('r'), region);
    }
    if let Some(access) = choice.switch(crate::endpoint::Toggle::Access) {
        hints = hints.key(Key::Alt('a'), access);
    }
    if choice.stored() {
        hints = hints.key(Key::Alt('d'), "sign out");
    }
    if stacked {
        return hints.key(Key::Esc, "back");
    }
    hints
        .key(Key::Esc, "close")
        .right(format!("{} / {}", selected + 1, rows.len()))
}

fn chip(choice: &AuthInfo, state: Option<&State>) -> String {
    state
        .filter(|state| state.failed() || choice.login.signs_in())
        .map_or_else(|| choice.state(), ToString::to_string)
}

fn listing(app: &App) -> Vec<NodeBuilder> {
    let source = app.clone();
    let rows = matching(app.auth.choices.get(), &app.auth.list.query.get());
    let selected = app.auth.list.cursor.get().min(rows.len().saturating_sub(1));
    let Some(choice) = rows.get(selected) else {
        return vec![group("No providers match.")];
    };
    let states = app.picker.groups.get();
    let (lookup, shown) = sections(&rows, &states, selected);
    let explain = canvas::is_window() && app.auth.list.query.get().is_empty();
    let (cols, term_rows) = app.viewport.get();
    let (split, span, visible) = browser_layout(
        (cols, canvas::picker_rows(term_rows)),
        SPLIT_COLS,
        (LIST_CHROME + u16::from(explain), NARROW_NOTE_ROWS as u16),
    );
    let state = chip(choice, states.get(&choice.name));
    let list = overlay_list(
        virtual_list(
            lookup,
            move |_, (name, chip, index): &(String, String, Option<usize>)| match index {
                Some(index) => entry(source.clone(), name, chip, *index, *index == selected),
                None => group(name.clone()),
            },
        )
        .scroll_bind(app.auth.list.scroll)
        .follow_row(signal(shown)),
        canvas::overlay_list_row(),
        visible,
    );
    if !split {
        let wide = span.saturating_sub(10);
        let note = match app.auth.step.get() {
            Login::List(note) => note,
            Login::Key(_) | Login::Checking | Login::SignIn { .. } | Login::Confirm => {
                Note::default()
            }
        };
        let mut below = notes(&note, wide);
        if below.is_empty() {
            below.push(group(crate::display::ellipsize(
                &detail::summary(choice, &state),
                wide,
            )));
        }
        let mut nodes = Vec::new();
        if explain {
            nodes.push(group(KEY_NOTE));
        }
        nodes.push(list);
        nodes.extend(below.into_iter().take(NARROW_NOTE_ROWS));
        return nodes;
    }
    let width = span.saturating_sub(LIST_CELLS as usize + 12);
    browser_columns(
        [list],
        LayoutStyle::flex_column().width(LIST_CELLS * canvas::cell().w),
        step_pane(app, width, visible),
        16.0,
    )
}

pub(super) fn step_pane(app: &App, width: usize, rows: usize) -> Vec<NodeBuilder> {
    let Login::List(note) = app.auth.step.get() else {
        return form(app, width);
    };
    let Some(choice) = app.auth_choice() else {
        return Vec::new();
    };
    let states = app.picker.groups.get();
    let tier = states.get(&choice.name).copied();
    let state = chip(&choice, states.get(&choice.name));
    let mut nodes = notes(&note, width);
    nodes.extend(detail::pane(
        app,
        &choice,
        (state.as_str(), tier),
        rows,
        width,
    ));
    nodes
}

type Row = (String, String, Option<usize>);

fn sections(
    rows: &[AuthInfo],
    states: &std::collections::BTreeMap<String, State>,
    selected: usize,
) -> (Vec<Row>, usize) {
    let mut lookup: Vec<Row> = Vec::new();
    let mut shown = 0;
    for (index, row) in rows.iter().enumerate() {
        let tier = section(row, states.get(&row.name).copied());
        if index == 0
            || tier != section(&rows[index - 1], states.get(&rows[index - 1].name).copied())
        {
            lookup.push((tier.title().to_string(), String::new(), None));
        }
        if index == selected {
            shown = lookup.len();
        }
        lookup.push((
            row.title().to_string(),
            chip(row, states.get(&row.name)),
            Some(index),
        ));
    }
    (lookup, shown)
}

fn entry(app: App, name: &str, chip: &str, index: usize, active: bool) -> NodeBuilder {
    let choose = app.clone();
    pick(active, name.to_string(), chip.to_string()).pointer_down(move |event| {
        choose.pick_auth(index);
        event.stop_propagation();
    })
}
