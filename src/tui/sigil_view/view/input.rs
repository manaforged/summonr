use super::*;

pub(super) fn composer_keys(mut node: NodeBuilder, app: &App) -> NodeBuilder {
    for modifier in [ModifiersState::SHIFT, ModifiersState::ALT] {
        node = node.key_capture(Shortcut::new(Key::Enter, modifier), |event| {
            rewrite(event, Key::Enter);
        });
    }
    for (key, end) in [(Key::Home, false), (Key::End, true)] {
        for extend in [false, true] {
            let app = app.clone();
            let modifiers = if extend {
                ModifiersState::CONTROL | ModifiersState::SHIFT
            } else {
                ModifiersState::CONTROL
            };
            node = node.key_capture(Shortcut::new(key, modifiers), move |event| {
                if app.boundary(end, extend) {
                    event.stop_propagation();
                }
            });
        }
    }
    node
}

pub(super) fn tab_shortcuts(node: NodeBuilder, app: &App) -> NodeBuilder {
    let complete = app.clone();
    let mode = app.clone();
    node.key_capture(
        Shortcut::new(Key::Tab, ModifiersState::empty()),
        move |event| {
            event.stop_propagation();
            if !complete.overlay_tab(1) && !complete.complete() {
                complete.cycle_effort(1);
            }
        },
    )
    .key_capture(
        Shortcut::new(Key::Tab, ModifiersState::SHIFT),
        move |event| {
            event.stop_propagation();
            if !mode.overlay_tab(-1) {
                drop(mode.cmd.send(Cmd::ToggleApproval));
            }
        },
    )
}

pub(super) fn leader_shortcuts(mut node: NodeBuilder, app: &App) -> NodeBuilder {
    for key in 'a'..='z' {
        let route = app.clone();
        node = node.key_capture(
            Shortcut::ctrl(LogicalKey::character(key.to_string())),
            move |event| {
                if apply_ctrl(&route, key, event) {
                    event.stop_propagation();
                }
            },
        );
    }
    node
}

pub(super) fn hotkey_shortcuts(mut node: NodeBuilder, app: &App) -> NodeBuilder {
    for key in 'a'..='z' {
        let route = app.clone();
        node = node.key_capture(
            Shortcut::ctrl(LogicalKey::character(key.to_string())),
            move |event| {
                if newline(&route, key) {
                    rewrite(event, Key::Enter);
                }
            },
        );
    }
    node
}

pub(super) fn chord_letter(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let key = chars.next()?.to_ascii_lowercase();
    (chars.next().is_none() && key.is_ascii_lowercase()).then_some(key)
}

pub(super) fn leader_plain(app: &App, event: &mut KeyEvent) -> bool {
    if event.modifiers.intersects(ModifiersState::CONTROL) || !app.hotkeys.armed.peek() {
        return false;
    }
    let raw = event.text.as_deref().or(match &event.key {
        LogicalKey::Character(value) => Some(value.as_str()),
        _non_character => None,
    });
    if let Some(key) = raw.and_then(chord_letter) {
        app.take_chord(key);
        true
    } else {
        app.hotkeys.armed.set(false);
        false
    }
}

fn newline(app: &App, key: char) -> bool {
    !app.hotkeys.recording.peek()
        && !app.hotkeys.armed.peek()
        && !app.options_active()
        && app.model.get().prefs.hotkeys.action(key) == Some(crate::prefs::Hotkey::Newline)
}

fn apply_ctrl(app: &App, key: char, event: &mut KeyEvent) -> bool {
    match ctrl_action(app, key) {
        Ctrl::Stop => true,
        Ctrl::Rewrite(key) => {
            rewrite(event, key);
            false
        }
        Ctrl::Fall => false,
    }
}

fn rewrite(event: &mut KeyEvent, key: Key) {
    event.key = key.into();
    event.modifiers = ModifiersState::empty();
    event.text = None;
}

fn ctrl_action(app: &App, key: char) -> Ctrl {
    if app.hotkeys.recording.peek() || app.hotkeys.armed.peek() {
        return held_ctrl(app, key);
    }
    if yields_cut(app, key) {
        return Ctrl::Fall;
    }
    if app.route_hotkey(key) {
        return Ctrl::Stop;
    }
    if key == 'x' {
        app.arm_leader();
        return Ctrl::Stop;
    }
    readline(app, key)
}

fn held_ctrl(app: &App, key: char) -> Ctrl {
    if app.hotkeys.armed.peek() {
        app.take_chord(key);
        return Ctrl::Stop;
    }
    if key == 'x' {
        app.arm_leader();
        return Ctrl::Stop;
    }
    app.record_chord(key, false);
    Ctrl::Stop
}

fn readline(app: &App, key: char) -> Ctrl {
    if let Some(motion) = motion(key) {
        return Ctrl::Rewrite(motion);
    }
    if key == 'd' && !app.draft.peek().is_empty() {
        return Ctrl::Rewrite(Key::Delete);
    }
    if key == 'l' && app.model.get().prefs.hotkeys.action('l').is_none() {
        app.refresh();
        return Ctrl::Stop;
    }
    Ctrl::Fall
}

fn motion(key: char) -> Option<Key> {
    match key {
        'a' => Some(Key::Home),
        'e' => Some(Key::End),
        'b' => Some(Key::ArrowLeft),
        'f' => Some(Key::ArrowRight),
        'p' => Some(Key::ArrowUp),
        'n' => Some(Key::ArrowDown),
        _other_key => None,
    }
}

fn yields_cut(app: &App, key: char) -> bool {
    key == 'x'
        && crate::prefs::EditorKey::yields(key, app.draft.peek().is_empty())
        && app.editor.selection().is_some()
}

enum Ctrl {
    Stop,
    Rewrite(Key),
    Fall,
}

pub(super) fn escape(app: &App) {
    if app.overlay_close() {
        return;
    }
    if app.model.get().busy_since.is_some() {
        drop(app.cmd.send(Cmd::Cancel));
    } else if app.dismiss() {
    } else if app.splash.peek() {
        app.splash.set(false);
    } else {
        app.escape_idle();
    }
}
