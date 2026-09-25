use super::*;

pub(in crate::tui::sigil_view) fn shell(app: &App) -> NodeBuilder {
    type Handler = fn(&App, &mut KeyEvent);
    let mut shell = container()
        .role(Role::Main)
        .label("Summonr agent harness")
        .layout(LayoutStyle::flex_column().fill_width().fill_height());
    let bindings: &[(Shortcut, Handler)] = &[
        (
            Shortcut::new(LogicalKey::character("y"), ModifiersState::SHIFT),
            |app, event| {
                if !app.options_active() && app.approval.peek().is_some_and(|(_, persist)| persist)
                {
                    event.stop_propagation();
                    app.approval_answer("always");
                }
            },
        ),
        (LogicalKey::character("y").into(), |app, event| {
            if !app.options_active() && app.approval.peek().is_some() {
                event.stop_propagation();
                app.approval_answer("y");
            }
        }),
        (LogicalKey::character("n").into(), |app, event| {
            if !app.options_active() && app.approval.peek().is_some() {
                event.stop_propagation();
                app.approval_answer("n");
            }
        }),
        (Key::F5.into(), |app, event| {
            if app.editing_model.peek() {
                event.stop_propagation();
                app.refresh_models();
                return;
            }
            if !app.options_active() && !app.view_edit.peek() {
                event.stop_propagation();
                app.retry();
            }
        }),
        (
            Shortcut::new(LogicalKey::character("p"), ModifiersState::ALT),
            pin_key,
        ),
        (LogicalKey::character("π").into(), pin_key),
        (
            Shortcut::new(LogicalKey::character("d"), ModifiersState::ALT),
            default_key,
        ),
        (LogicalKey::character("∂").into(), default_key),
        (Key::Escape.into(), |app, event| {
            event.stop_propagation();
            if app.options_active() {
                super::input::escape(app);
            } else if app.edit_stop() {
            } else if app.context.peek().is_some() {
                app.context_close();
            } else if app.pane_back() {
            } else if app.pane_focus.peek() {
                app.focus_pane(false);
            } else {
                super::input::escape(app);
            }
        }),
        (LogicalKey::character("e").into(), |app, event| {
            if app.pane_active() == Some(super::Pane::View) {
                event.stop_propagation();
                app.edit_start();
            }
        }),
        (Shortcut::ctrl(LogicalKey::character("s")), |app, event| {
            if app.editing_model.peek() {
                event.stop_propagation();
            } else if app.view_edit.peek() && !app.hotkey_recording.peek() {
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
        (LogicalKey::character("o").into(), |app, event| {
            if app.pane_active() == Some(super::Pane::View) {
                event.stop_propagation();
                app.view_external();
            }
        }),
        (Key::Home.into(), |app, event| {
            if app.overlay() == Some(Overlay::Usage) {
                return;
            }
            if app.pane_active().is_some() {
                event.stop_propagation();
                app.pane_edge(false);
            } else if app.overlay().is_none() && app.draft.peek().is_empty() {
                event.stop_propagation();
                app.history_edge(false);
            }
        }),
        (Key::End.into(), |app, event| {
            if app.overlay() == Some(Overlay::Usage) {
                return;
            }
            if app.pane_active().is_some() {
                event.stop_propagation();
                app.pane_edge(true);
            } else if app.overlay().is_none() && app.draft.peek().is_empty() {
                event.stop_propagation();
                app.history_edge(true);
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
        (Key::Enter.into(), |app, event| {
            if app.view_edit.peek() && !app.options_active() {
                return;
            }
            event.stop_propagation();
            if app.overlay_enter() {
            } else if app.context.peek().is_some() {
                app.context_enter();
            } else if app.pane_active().is_some() {
                app.pane_enter();
            } else if !app.accept() {
                app.submit();
            }
        }),
        (Shortcut::ctrl(Key::ArrowLeft), |app, event| {
            event.stop_propagation();
            app.focus_pane(false);
        }),
        (Shortcut::ctrl(Key::ArrowRight), |app, event| {
            event.stop_propagation();
            app.focus_pane(true);
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
        (Shortcut::ctrl(LogicalKey::character("c")), |app, event| {
            if !app.hotkey_recording.peek() {
                event.stop_propagation();
                app.interrupt();
            }
        }),
        (Shortcut::ctrl(LogicalKey::character("d")), |app, event| {
            if !app.hotkey_recording.peek()
                && !app.options_active()
                && app.draft.peek().is_empty()
                && app.model.get().busy_since.is_none()
            {
                event.stop_propagation();
                app.quit.set(true);
            }
        }),
        (Key::PageUp.into(), |app, event| {
            if app.overlay() == Some(Overlay::Usage) {
                return;
            }
            event.stop_propagation();
            if app.overlay_move(-8) {
                return;
            }
            if app.pane_active().is_some() {
                app.pane_page(-1);
            } else {
                app.page_history(-1.0);
            }
        }),
        (Key::PageDown.into(), |app, event| {
            if app.overlay() == Some(Overlay::Usage) {
                return;
            }
            event.stop_propagation();
            if app.overlay_move(8) {
                return;
            }
            if app.pane_active().is_some() {
                app.pane_page(1);
            } else {
                app.page_history(1.0);
            }
        }),
        (Key::ArrowUp.into(), |app, event| {
            if app.overlay() == Some(Overlay::Usage) {
                return;
            }
            if app.overlay_move(-1) {
                event.stop_propagation();
            } else if app.context.peek().is_some() {
                event.stop_propagation();
                app.context_move(-1);
            } else if app.pane_active().is_some() {
                event.stop_propagation();
                app.pane_move(-1);
            } else if app.draft.peek().is_empty() && app.model.get().queued != (0, 0) {
                event.stop_propagation();
                app.dequeue();
            } else if app.suggesting() {
                event.stop_propagation();
                app.shift(-1);
            }
        }),
        (Key::ArrowDown.into(), |app, event| {
            if app.overlay() == Some(Overlay::Usage) {
                return;
            }
            if app.overlay_move(1) {
                event.stop_propagation();
            } else if app.context.peek().is_some() {
                event.stop_propagation();
                app.context_move(1);
            } else if app.pane_active().is_some() {
                event.stop_propagation();
                app.pane_move(1);
            } else if app.suggesting() {
                event.stop_propagation();
                app.shift(1);
            }
        }),
        (Key::ArrowLeft.into(), |app, event| {
            if app.overlay_adjust(-1) {
                event.stop_propagation();
            }
        }),
        (Key::ArrowRight.into(), |app, event| {
            if app.overlay_adjust(1) {
                event.stop_propagation();
            }
        }),
        (LogicalKey::character("d").into(), |app, event| {
            if app.workflow_keys() {
                event.stop_propagation();
                app.clear_workflow_seat();
            }
        }),
        (LogicalKey::character("s").into(), |app, event| {
            if app.workflow_keys() {
                event.stop_propagation();
                app.begin_workflow_save();
            }
        }),
        (LogicalKey::character(" ").into(), |app, event| {
            if app.overlay() == Some(Overlay::Settings) {
                event.stop_propagation();
                app.adjust_setting();
            }
        }),
        (LogicalKey::character("+").into(), |app, event| {
            if app.overlay() == Some(Overlay::Settings) {
                event.stop_propagation();
                app.overlay_adjust(1);
            }
        }),
        (LogicalKey::character("-").into(), |app, event| {
            if app.overlay() == Some(Overlay::Settings) {
                event.stop_propagation();
                app.overlay_adjust(-1);
            }
        }),
        (LogicalKey::character("r").into(), |app, event| {
            if app.overlay() == Some(Overlay::Hotkeys) && !app.hotkey_recording.peek() {
                event.stop_propagation();
                app.reset_hotkeys();
            }
        }),
    ];
    for (shortcut, action) in bindings {
        let app = app.clone();
        let action = *action;
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

fn pin_key(app: &App, event: &mut KeyEvent) {
    if app.editing_model.peek() {
        event.stop_propagation();
        app.pin_model();
    }
}

fn default_key(app: &App, event: &mut KeyEvent) {
    if app.editing_model.peek() {
        event.stop_propagation();
        if app.model_for.peek().is_none() {
            app.default_model();
        }
    } else if app.overlay() == Some(Overlay::Sessions) {
        event.stop_propagation();
        app.delete_session();
    } else if app.overlay() == Some(Overlay::Auth) {
        event.stop_propagation();
        app.confirm_logout();
    }
}
