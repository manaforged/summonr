use sigil::prelude::KeyPhase;

use super::*;

pub(super) fn intake(app: &App, events: &mut Vec<ShellEvent>) {
    let mut staged = false;
    for event in std::mem::take(events) {
        let ShellEvent::Key(key) = &event else {
            events.push(event);
            continue;
        };
        if key.phase != KeyPhase::Down {
            if !app.secret_active() {
                events.push(event);
            }
        } else if app.secret_active() {
            secret(app, key);
        } else if let Some(text) = pasted(key) {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            match if staged {
                Some(true)
            } else {
                stage(app, &text)
            } {
                Some(true) => staged = true,
                Some(false) => {}
                None => events.push(sigil::terminal::paste(&text)),
            }
        } else if staged && is_enter(key) {
        } else if paste_chord(key) && (staged || app.clipboard()) {
            staged = true;
        } else {
            events.push(event);
        }
    }
}

fn secret(app: &App, key: &sigil::prelude::KeyEvent) {
    let chord = key
        .modifiers
        .intersects(ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::META);
    if key.key == LogicalKey::Named(Key::Enter) {
        app.secret_finish(true);
    } else if key.key == LogicalKey::Named(Key::Escape)
        || (key.key == LogicalKey::character("c")
            && key.modifiers.contains(ModifiersState::CONTROL))
    {
        app.secret_finish(false);
    } else if key.key == LogicalKey::Named(Key::Backspace) {
        app.secret_pop();
    } else if let Some(text) = key.text.as_deref()
        && !chord
    {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        app.secret_push(text.lines().next().unwrap_or_default());
    }
}

fn stage(app: &App, text: &str) -> Option<bool> {
    if app.pane_active().is_some() && !app.options_active() {
        app.focus_pane(false);
    }
    if text.trim().is_empty() {
        return Some(app.clipboard());
    }
    if app.paste(text) || (graphics_junk(text) && app.clipboard()) {
        return Some(true);
    }
    app.paste_text(text).then_some(false)
}

fn pasted(key: &sigil::prelude::KeyEvent) -> Option<&str> {
    let text = key.text.as_deref()?;
    (text.chars().count() > 1 || text.contains(['\n', '\r'])).then_some(text)
}

fn is_enter(key: &sigil::prelude::KeyEvent) -> bool {
    key.key == LogicalKey::Named(Key::Enter) && key.modifiers.is_empty()
}

fn paste_chord(key: &sigil::prelude::KeyEvent) -> bool {
    key.key == LogicalKey::character("v")
        && key
            .modifiers
            .intersects(ModifiersState::CONTROL | ModifiersState::META)
        && !key.modifiers.contains(ModifiersState::ALT)
}

fn graphics_junk(text: &str) -> bool {
    text.chars()
        .any(|ch| ch == '\u{1b}' || (ch.is_control() && !matches!(ch, '\n' | '\t' | '\r')))
}
