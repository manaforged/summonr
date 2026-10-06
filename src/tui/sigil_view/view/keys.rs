use super::*;

type Handler = fn(&App, &mut KeyEvent);

fn escape(app: &App, event: &mut KeyEvent) {
    event.stop_propagation();
    if app.options_active() {
        super::input::escape(app);
    } else if app.form_stage().is_some() {
        app.form_reply(crate::tui::sigil_view::app::Reply::Cancel);
    } else if app.edit_stop() || app.note_stop() {
    } else if app.context.peek().is_some() {
        app.context_close();
    } else if app.pane_back() {
    } else if app.pane_focus.peek() {
        app.focus_pane(false);
    } else {
        super::input::escape(app);
    }
}

fn enter(app: &App, event: &mut KeyEvent) {
    if (app.view_edit.peek() || app.note_target.peek().is_some()) && !app.options_active() {
        return;
    }
    event.stop_propagation();
    if app.overlay_enter() {
    } else if app.context.peek().is_some() {
        app.context_enter();
    } else if app.pane_active().is_some() {
        app.pane_enter();
    } else if app.form_enter() {
    } else if !app.accept() {
        app.submit();
    }
}

fn review(app: &App) -> bool {
    !app.options_active() && app.form_stage() == Some(crate::tui::sigil_view::app::Stage::Review)
}

fn answering(app: &App) -> bool {
    !app.options_active() && app.draft.peek().is_empty() && app.approval.peek().is_some()
}

fn shortcuts(app: &App, event: &mut KeyEvent) {
    if !app.options_active()
        && !app.view_edit.peek()
        && !app.pane_focus.peek()
        && app.form_stage().is_none()
        && app.approval.peek().is_none()
        && app.draft.peek().is_empty()
    {
        event.stop_propagation();
        app.help.show();
        app.refresh();
    }
}

fn respond() -> Vec<(Shortcut, Handler)> {
    vec![
        (LogicalKey::character("?").into(), shortcuts),
        (
            Shortcut::new(LogicalKey::character("?"), ModifiersState::SHIFT),
            shortcuts,
        ),
        (
            Shortcut::new(LogicalKey::character("y"), ModifiersState::SHIFT),
            |app, event| {
                if answering(app) && app.approval.peek().is_some_and(|(_, persist)| persist) {
                    event.stop_propagation();
                    app.approval_answer(crate::runtime::GrantAnswer::Always);
                }
            },
        ),
        (LogicalKey::character("y").into(), |app, event| {
            if review(app) {
                event.stop_propagation();
                app.form_reply(crate::tui::sigil_view::app::Reply::Send);
            } else if answering(app) {
                event.stop_propagation();
                app.approval_answer(crate::runtime::GrantAnswer::Allow);
            }
        }),
        (LogicalKey::character("n").into(), |app, event| {
            if review(app) {
                event.stop_propagation();
                app.form_reply(crate::tui::sigil_view::app::Reply::Decline);
            } else if answering(app) {
                event.stop_propagation();
                app.approval_answer(crate::runtime::GrantAnswer::Deny);
            }
        }),
        (Key::F5.into(), |app, event| {
            if !app.options_active() && !app.view_edit.peek() {
                event.stop_propagation();
                app.retry();
            }
        }),
        (Key::Escape.into(), escape),
        (Key::Enter.into(), enter),
        (Shortcut::ctrl(LogicalKey::character("c")), |app, event| {
            if !app.hotkeys.recording.peek() {
                event.stop_propagation();
                app.interrupt();
            }
        }),
        (Shortcut::ctrl(LogicalKey::character("d")), |app, event| {
            if !app.hotkeys.recording.peek()
                && !app.options_active()
                && app.draft.peek().is_empty()
                && app.model.get().busy_since.is_none()
            {
                event.stop_propagation();
                app.quit.set(true);
            }
        }),
    ]
}

fn panes() -> Vec<(Shortcut, Handler)> {
    vec![
        (LogicalKey::character("e").into(), |app, event| {
            if review(app) {
                event.stop_propagation();
                app.form_reply(crate::tui::sigil_view::app::Reply::Edit);
            } else if app.pane_active() == Some(super::Pane::View) {
                event.stop_propagation();
                app.edit_start();
            }
        }),
        (Shortcut::ctrl(LogicalKey::character("s")), |app, event| {
            if app.picker.open.peek() {
                event.stop_propagation();
            } else if app.view_edit.peek() && !app.hotkeys.recording.peek() {
                event.stop_propagation();
                app.edit_save();
            }
        }),
        (LogicalKey::character("t").into(), |app, event| {
            if app.pane_active() == Some(super::Pane::Diff) {
                event.stop_propagation();
                app.toggle_recent();
            }
        }),
        (LogicalKey::character("c").into(), |app, event| {
            if app.pane_active() == Some(super::Pane::Diff) {
                event.stop_propagation();
                app.copy_patch();
            }
        }),
        (LogicalKey::character("r").into(), |app, event| {
            if app.pane_active() == Some(super::Pane::Diff) && app.diff_view.peek().is_some() {
                event.stop_propagation();
                app.hunk_reject();
            }
        }),
        (LogicalKey::character("n").into(), |app, event| {
            if app.pane_active() == Some(super::Pane::Diff)
                && let Some(path) = app.diff_view.peek()
            {
                let hunks = app.hunks();
                let Some(hunk) =
                    hunks.get(app.hunk_cursor.peek().min(hunks.len().saturating_sub(1)))
                else {
                    return;
                };
                event.stop_propagation();
                app.note_start(path, hunk.id.clone());
            }
        }),
        (LogicalKey::character("o").into(), |app, event| {
            if app.pane_active() == Some(super::Pane::View) {
                event.stop_propagation();
                app.view_external();
            }
        }),
        (Key::ArrowLeft.into(), |app, event| {
            if app.pane_active().is_some() {
                event.stop_propagation();
                app.pane_collapse();
            }
        }),
        (Key::ArrowRight.into(), |app, event| {
            if app.pane_active().is_some() {
                event.stop_propagation();
                app.pane_expand();
            }
        }),
        (Shortcut::ctrl(Key::ArrowLeft), |app, event| {
            event.stop_propagation();
            app.tab_or_pane(false);
        }),
        (Shortcut::ctrl(Key::ArrowRight), |app, event| {
            event.stop_propagation();
            app.tab_or_pane(true);
        }),
        (
            Shortcut::new(
                Key::ArrowLeft,
                ModifiersState::CONTROL | ModifiersState::SHIFT,
            ),
            |app, event| {
                event.stop_propagation();
                app.nudge_split(-5);
            },
        ),
        (
            Shortcut::new(
                Key::ArrowRight,
                ModifiersState::CONTROL | ModifiersState::SHIFT,
            ),
            |app, event| {
                event.stop_propagation();
                app.nudge_split(5);
            },
        ),
    ]
}

fn edge(app: &App, event: &mut KeyEvent, end: bool) {
    if app.overlay() == Some(Overlay::Usage) {
        return;
    }
    if app.pane_active().is_some() {
        event.stop_propagation();
        app.pane_edge(end);
    } else if app.overlay().is_none() && app.draft.peek().is_empty() {
        event.stop_propagation();
        app.history_edge(end);
    }
}

fn first(app: &App, event: &mut KeyEvent) {
    edge(app, event, false);
}

fn last(app: &App, event: &mut KeyEvent) {
    edge(app, event, true);
}

fn ctrl_edge(app: &App, event: &mut KeyEvent, end: bool) {
    if app.overlay().is_none() {
        event.stop_propagation();
        app.history_edge(end);
    }
}

fn ctrl_first(app: &App, event: &mut KeyEvent) {
    ctrl_edge(app, event, false);
}

fn ctrl_last(app: &App, event: &mut KeyEvent) {
    ctrl_edge(app, event, true);
}

fn page(app: &App, event: &mut KeyEvent, dir: isize) {
    if app.overlay() == Some(Overlay::Usage) {
        return;
    }
    event.stop_propagation();
    if app.overlay_move(8 * dir) {
        return;
    }
    if app.pane_active().is_some() {
        app.pane_page(dir);
    } else {
        app.page_history(dir as f32);
    }
}

fn page_up(app: &App, event: &mut KeyEvent) {
    page(app, event, -1);
}

fn page_down(app: &App, event: &mut KeyEvent) {
    page(app, event, 1);
}

fn arrow(app: &App, event: &mut KeyEvent, dir: isize) {
    if app.overlay() == Some(Overlay::Usage) {
        return;
    }
    if app.overlay_move(dir) {
        event.stop_propagation();
    } else if app.context.peek().is_some() {
        event.stop_propagation();
        app.context_move(dir);
    } else if app.pane_active().is_some() {
        event.stop_propagation();
        app.pane_move(dir);
    } else if dir < 0 && app.draft.peek().is_empty() && app.model.get().queued != (0, 0) {
        event.stop_propagation();
        app.dequeue();
    } else if app.suggesting() {
        event.stop_propagation();
        app.shift(dir);
    }
}

fn up(app: &App, event: &mut KeyEvent) {
    arrow(app, event, -1);
}

fn down(app: &App, event: &mut KeyEvent) {
    arrow(app, event, 1);
}

fn adjust(app: &App, event: &mut KeyEvent, dir: isize) {
    if app.overlay_adjust(dir) {
        event.stop_propagation();
    }
}

fn left(app: &App, event: &mut KeyEvent) {
    adjust(app, event, -1);
}

fn right(app: &App, event: &mut KeyEvent) {
    adjust(app, event, 1);
}

fn nav() -> Vec<(Shortcut, Handler)> {
    vec![
        (Key::Home.into(), first),
        (Key::End.into(), last),
        (Shortcut::ctrl(Key::Home), ctrl_first),
        (Shortcut::ctrl(Key::End), ctrl_last),
        (Key::PageUp.into(), page_up),
        (Key::PageDown.into(), page_down),
        (Key::ArrowUp.into(), up),
        (Key::ArrowDown.into(), down),
        (Key::ArrowLeft.into(), left),
        (Key::ArrowRight.into(), right),
    ]
}

pub(in crate::tui::sigil_view) fn shell(app: &App) -> NodeBuilder {
    let mut shell = container()
        .role(Role::Main)
        .label("Summonr agent harness")
        .layout(LayoutStyle::flex_column().fill_width().fill_height());
    let bindings = respond().into_iter().chain(panes()).chain(nav());
    for (shortcut, action) in bindings {
        let app = app.clone();
        shell = shell.key_capture(shortcut.clone(), move |event| action(&app, event));
    }
    let older = app.clone();
    let newer = app.clone();
    let shell = shell
        .key(Key::ArrowUp, move |event| {
            if older.recall(true) {
                event.stop_propagation();
            }
        })
        .key(Key::ArrowDown, move |event| {
            if newer.recall(false) {
                event.stop_propagation();
            }
        });
    let scroll = app.clone();
    let shell = shell
        .child(super::input::tab_shortcuts(app))
        .on_scroll(move |event| {
            let lines = super::canvas::scroll_lines(event.delta);
            if scroll.overlay_move(-lines.round() as isize) {
                event.stop_propagation();
                return;
            }
            if !scroll.options_active()
                && !scroll.splash.peek()
                && scroll.layout.peek().secondary.is_some()
                && event.position.x >= f32::from(scroll.chat_cols()) * super::canvas::cell().w
            {
                scroll.pane_move(-lines.round() as isize);
                event.stop_propagation();
                return;
            }
            let delta = lines * super::canvas::cell().h;
            let before = scroll.scroll_y.peek();
            scroll.scroll_history(delta);
            if scroll.scroll_y.peek() != before {
                event.stop_propagation();
            }
        });
    super::input::hotkey_shortcuts(super::input::composer_keys(shell, app), app)
}
