use super::*;
use summonr_agent::prefs::EditorKey;

pub(super) fn composer_capture(app: &App, event: &mut KeyEvent) -> bool {
    for modifier in [ModifiersState::SHIFT, ModifiersState::ALT] {
        if Shortcut::new(Key::Enter, modifier).matches(event) {
            rewrite(event, Key::Enter);
            return false;
        }
    }
    for (key, end) in [(Key::Home, false), (Key::End, true)] {
        for extend in [false, true] {
            let modifiers = if extend {
                ModifiersState::CONTROL | ModifiersState::SHIFT
            } else {
                ModifiersState::CONTROL
            };
            if Shortcut::new(key, modifiers).matches(event) {
                return app.target().boundary(end, extend);
            }
        }
    }
    false
}

pub(super) fn tab_capture(app: &App, event: &mut KeyEvent) -> bool {
    if Shortcut::plain(Key::Tab).matches(event) {
        let complete = app.target();
        let composer = !complete.form_hidden() && (complete.complete() || complete.queue_draft());
        if complete.overlay_tab(1) || composer {
            return true;
        }
        if !canvas::is_window() {
            complete.cycle_effort(1);
            return true;
        }
    } else if Shortcut::new(Key::Tab, ModifiersState::SHIFT).matches(event) {
        let mode = app.target();
        if mode.overlay_tab(-1) {
            return true;
        }
        if !canvas::is_window() {
            drop(mode.cmd.send(Cmd::ToggleApproval));
            return true;
        }
    }
    false
}

pub(super) fn ctrl_letter(event: &KeyEvent) -> Option<char> {
    if event.normalized_modifiers() != ModifiersState::CONTROL {
        return None;
    }
    let LogicalKey::Character(value) = &event.key else {
        return None;
    };
    let mut chars = value.chars();
    let key = chars.next()?;
    (chars.next().is_none() && key.is_ascii_lowercase()).then_some(key)
}

pub(super) fn leader_capture(app: &App, event: &mut KeyEvent) -> bool {
    ctrl_letter(event).is_some_and(|key| apply_ctrl(&app.target(), key, event))
}

pub(super) fn hotkey_newline(app: &App, event: &mut KeyEvent) {
    if let Some(key) = ctrl_letter(event)
        && newline(&app.target(), key)
    {
        rewrite(event, Key::Enter);
    }
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
        && app.model.get().prefs.hotkeys.action(key) == Some(summonr_agent::prefs::Hotkey::Newline)
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
    if key == 'x' || EditorKey::yields(key, false) {
        return Ctrl::Fall;
    }
    if app.route_hotkey(key) {
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

enum Ctrl {
    Stop,
    Rewrite(Key),
    Fall,
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

pub(super) fn escape(app: &App) {
    if app.overlay_close() {
        return;
    }
    if app.dismiss() {
    } else if app.model.get().busy_since.is_some() {
        if !crate::canvas::is_window() {
            drop(app.cmd.send(Cmd::Cancel));
        }
    } else if app.splash.peek() {
        app.splash.set(false);
    } else {
        app.escape_idle();
    }
}
