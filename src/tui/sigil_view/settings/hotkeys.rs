use sigil::prelude::{NodeBuilder, Tone};

use super::{App, canvas};
use crate::prefs::Hotkey;
use crate::tui::sigil_view::ui::{Bindings, Dialog, Hints, Key, Size, notice, pick, press};

pub(in crate::tui::sigil_view) fn keys() -> Bindings {
    vec![press("r", |app, event| {
        if !app.hotkeys.recording.peek() {
            event.stop_propagation();
            app.reset_hotkeys();
        }
    })]
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    let state = app.clone();
    let choose = app.clone();
    let viewport = app.viewport;
    let hotkeys = app.model.peek().prefs.hotkeys.clone();
    Dialog::new(Size::Reading, "Hotkeys", Hints::new())
        .live_hints(move || {
            if state.hotkeys.armed.get() {
                Hints::new()
                    .key(Key::Fixed("letter"), "bind")
                    .key(Key::Esc, "cancel")
            } else if state.hotkeys.recording.get() {
                Hints::new()
                    .key(Key::Fixed("ctrl+x, then a letter"), "bind")
                    .key(Key::Esc, "cancel")
            } else {
                Hints::new()
                    .key(Key::UpDown, "move")
                    .key(Key::Enter, "rebind")
                    .key(Key::Fixed("r"), "reset all")
                    .key(Key::Esc, "close")
            }
        })
        .child(super::dialog_body({
            let note = app.hotkeys.note;
            move || match note.get() {
                text if text.is_empty() => Vec::new(),
                text => vec![notice(text, Tone::Error)],
            }
        }))
        .collection(
            app.hotkeys.list,
            move || {
                let selected = paint.hotkeys.list.cursor.get().min(Hotkey::ALL.len() - 1);
                let recording = paint.hotkeys.recording.get();
                let armed = paint.hotkeys.armed.get();
                let model = paint.model.get();
                Ok(Hotkey::ALL
                    .iter()
                    .copied()
                    .map(|action| {
                        let binding = if recording && action == Hotkey::ALL[selected] {
                            if armed {
                                "press a letter".to_string()
                            } else {
                                "press ctrl+letter or ctrl+x, then a letter".to_string()
                            }
                        } else {
                            model.prefs.hotkeys.label(action)
                        };
                        (action, binding)
                    })
                    .collect())
            },
            String::new,
            move |len| {
                Some((
                    canvas::overlay_list_row(),
                    canvas::overlay_shown(len, viewport.get().1),
                ))
            },
            |(action, binding), active| {
                pick(
                    active,
                    action.name(),
                    format!("{binding} · {}", description(*action)),
                )
            },
            move || {
                choose.hotkeys.recording.set(false);
                choose.hotkeys.armed.set(false);
                choose.edit_hotkey();
            },
        )
        .build(app.hotkeys.open, app.viewport.peek().0, &hotkeys)
}

const fn description(action: Hotkey) -> &'static str {
    match action {
        Hotkey::Options => "open settings",
        Hotkey::Newline => "insert newline",
        Hotkey::Editor => "open $EDITOR",
        Hotkey::Files => "toggle files",
        Hotkey::Ledger => "expand or collapse tasks",
        Hotkey::Agents => "expand or collapse children",
        Hotkey::Diff => "toggle diff",
        Hotkey::Copy => "copy last reply",
        Hotkey::Select => "select transcript text",
        Hotkey::Pane => "hide/show pane",
        Hotkey::Search => "search conversation",
        Hotkey::History => "search saved prompts",
        Hotkey::Fold => "fold thinking, then tool output",
    }
}
