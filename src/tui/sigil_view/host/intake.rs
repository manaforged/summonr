use super::terminal::Terminal;
use super::*;

pub(in crate::tui::sigil_view) use crate::limits::ANIMATION;

pub(in crate::tui::sigil_view) fn frame_budget(fps: u32) -> Duration {
    let fps = fps.clamp(crate::prefs::FPS_MIN, crate::prefs::FPS_MAX);
    crate::limits::FRAME_WINDOW / fps
}

pub(in crate::tui::sigil_view) fn is_enter(event: &ShellEvent) -> bool {
    matches!(
        event,
        ShellEvent::Key(key)
            if key.key == LogicalKey::Named(Key::Enter) && key.modifiers.is_empty()
    )
}

pub(in crate::tui::sigil_view) fn paste_chord(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('v')
        && key.kind == KeyEventKind::Press
        && key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
        && !key.modifiers.contains(KeyModifiers::ALT)
}

pub(in crate::tui::sigil_view) fn graphics_junk(text: &str) -> bool {
    text.chars()
        .any(|ch| ch == '\u{1b}' || (ch.is_control() && !matches!(ch, '\n' | '\t' | '\r')))
}

pub(in crate::tui::sigil_view) fn finish_intake(
    incoming: &mut Vec<ShellEvent>,
    staged: bool,
    term: &mut Terminal,
) {
    if staged {
        incoming.retain(|event| !is_enter(event));
        term.redraw = true;
    }
}

pub(in crate::tui::sigil_view) fn read_event(
    event: TermEvent,
    incoming: &mut Vec<ShellEvent>,
    pointer: &mut Point,
    screen: &mut Screen,
    term: &mut Terminal,
    app: &App,
    already: bool,
) -> Result<bool> {
    match event {
        TermEvent::Key(key) if app.secret_active() => {
            if key.kind != KeyEventKind::Release {
                let chord = key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
                if key.code == KeyCode::Enter {
                    app.secret_finish(true);
                } else if key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    app.secret_finish(false);
                } else if key.code == KeyCode::Backspace {
                    app.secret_pop();
                } else if let KeyCode::Char(ch) = key.code
                    && !chord
                {
                    app.secret_push(ch.encode_utf8(&mut [0; 4]));
                }
            }
            term.redraw = true;
        }
        TermEvent::Paste(text) if app.secret_active() => {
            app.secret_push(text.trim_end_matches(['\r', '\n']));
            term.redraw = true;
        }
        TermEvent::Key(key) => {
            if key.code == KeyCode::Char('c')
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && let Some(text) = term.selection.as_ref().and_then(Selection::text)
            {
                app.copied(&text, term.copy(&text)?);
            } else if paste_chord(&key) && already {
                term.deselect(screen);
                return Ok(true);
            } else if paste_chord(&key) && app.clipboard() {
                term.deselect(screen);
                term.redraw = true;
                return Ok(true);
            } else {
                term.deselect(screen);
                incoming.extend(sigil_tui::map_key(&key));
            }
        }
        TermEvent::Resize(next_cols, next_rows) => {
            term.deselect(screen);
            app.resize(next_cols, next_rows);
            screen.invalidate();
            term.redraw = true;
        }
        TermEvent::Mouse(mouse) => {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(selection) = &mut term.selection {
                        selection.move_to(mouse.column, mouse.row);
                        term.redraw = true;
                        return Ok(false);
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(selection) = &mut term.selection {
                        selection.move_to(mouse.column, mouse.row);
                        if let Some(text) = selection.text() {
                            app.copied(&text, term.copy(&text)?);
                            term.redraw = true;
                        }
                        return Ok(false);
                    }
                }
                MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
                | MouseEventKind::ScrollLeft
                | MouseEventKind::ScrollRight
                | MouseEventKind::Down(_) => term.deselect(screen),
                _ => {}
            }
            incoming.extend(sigil_tui::map_mouse(&mouse, Host::CELL, pointer));
        }
        TermEvent::Paste(text) => {
            return take_paste(text, incoming, screen, term, app, already);
        }
        TermEvent::FocusGained => term.focused = true,
        TermEvent::FocusLost => term.focused = false,
    }
    Ok(false)
}

pub(in crate::tui::sigil_view) fn take_paste(
    text: String,
    incoming: &mut Vec<ShellEvent>,
    screen: &mut Screen,
    term: &mut Terminal,
    app: &App,
    already: bool,
) -> Result<bool> {
    term.deselect(screen);
    if already {
        return Ok(true);
    }
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if app.pane_active().is_some() && !app.options_active() {
        app.focus_pane(false);
    }
    if text.trim().is_empty() {
        if app.clipboard() {
            term.redraw = true;
            return Ok(true);
        }
        return Ok(false);
    }
    if app.paste(&text) {
        term.redraw = true;
        return Ok(true);
    }
    if graphics_junk(&text) && app.clipboard() {
        term.redraw = true;
        return Ok(true);
    }
    if app.paste_text(&text) {
        return Ok(false);
    }
    *term.paste.borrow_mut() = Some(text);
    incoming.extend(sigil_tui::map_key(&KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL,
    )));
    Ok(false)
}

pub(in crate::tui::sigil_view) fn frame<'a>(
    screen: &'a mut Screen,
    host: &mut Host,
    router: &mut EventRouter,
    incoming: impl IntoIterator<Item = ShellEvent>,
    size: (u16, u16),
) -> Result<&'a Grid> {
    screen
        .frame(host, router, incoming, size)
        .map_err(|error| Error::Tool(format!("Sigil layout failed: {error}")))
}
