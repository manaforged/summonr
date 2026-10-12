use sigil::prelude::KeyPhase;

use super::*;

impl App {
    pub(super) fn intake(&self, events: &mut Vec<ShellEvent>) {
        let mut staged = false;
        for event in std::mem::take(events) {
            if let Some(text) = pasted(&event) {
                let text = text.replace("\r\n", "\n").replace('\r', "\n");
                if self.secret_active() {
                    self.secret_push(text.lines().next().unwrap_or_default());
                } else if !staged {
                    match self.stage(&text) {
                        Some(true) => staged = true,
                        Some(false) => {}
                        None => events.push(sigil::terminal::paste(&text)),
                    }
                }
                continue;
            }
            let ShellEvent::Key(key) = &event else {
                events.push(event);
                continue;
            };
            if key.phase != KeyPhase::Down {
                if !self.secret_active() {
                    events.push(event);
                }
            } else if self.secret_active() {
                self.secret(key);
            } else if staged && is_enter(key) {
            } else if paste_chord(key) && (staged || self.clipboard()) {
                staged = true;
            } else {
                events.push(event);
            }
        }
    }

    fn secret(&self, key: &sigil::prelude::KeyEvent) {
        let chord = key
            .modifiers
            .intersects(ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::META);
        if key.key == LogicalKey::Named(Key::Enter) {
            self.secret_finish(true);
        } else if key.key == LogicalKey::Named(Key::Escape)
            || (key.key == LogicalKey::character("c")
                && key.modifiers.contains(ModifiersState::CONTROL))
        {
            self.secret_finish(false);
        } else if key.key == LogicalKey::Named(Key::Backspace) {
            self.secret_pop();
        } else if let Some(text) = key.text.as_deref()
            && !chord
        {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            self.secret_push(text.lines().next().unwrap_or_default());
        }
    }

    fn stage(&self, text: &str) -> Option<bool> {
        if self.pane_active().is_some() && !self.options_active() {
            self.focus_pane(false);
        }
        if text.trim().is_empty() {
            return Some(self.clipboard());
        }
        if self.paste(text) || (graphics_junk(text) && self.clipboard()) {
            return Some(true);
        }
        self.paste_text(text).then_some(false)
    }
}

fn pasted(event: &ShellEvent) -> Option<&str> {
    if let ShellEvent::Paste(text) = event {
        return Some(text);
    }
    let ShellEvent::Key(key) = event else {
        return None;
    };
    if key.phase != KeyPhase::Down {
        return None;
    }
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
            .intersects(ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::META)
}

fn graphics_junk(text: &str) -> bool {
    text.chars()
        .any(|ch| ch == '\u{1b}' || (ch.is_control() && !matches!(ch, '\n' | '\t' | '\r')))
}

#[cfg(all(test, target_os = "macos"))]
#[path = "intake_tests.rs"]
mod tests;
