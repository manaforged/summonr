use super::*;

type Handler = fn(&App, &mut KeyEvent);

impl App {
    fn escape(&self, event: &mut KeyEvent) {
        event.stop_propagation();
        if self.options_active() {
            super::input::escape(self);
        } else if self.form_stage().is_some() {
            self.form_reply(crate::tui::sigil_view::app::Reply::Cancel);
        } else if self.edit_stop() || self.note_stop() {
        } else if self.context.peek().is_some() {
            self.context_close();
        } else if self.pane_focus.peek() {
            self.focus_pane(false);
        } else if self.pane_back() {
        } else {
            super::input::escape(self);
        }
    }

    fn enter(&self, event: &mut KeyEvent) {
        if self.note_target.peek().is_some() && !self.options_active() {
            event.stop_propagation();
            self.note_commit(&self.note_text.peek());
            return;
        }
        if self.view_edit.peek() && !self.options_active() {
            return;
        }
        event.stop_propagation();
        if self.overlay_enter() {
        } else if self.context.peek().is_some() {
            self.context_enter();
        } else if self.pane_active().is_some() {
            self.pane_enter();
        } else if self.form_enter() {
        } else if !self.accept() {
            self.submit();
        }
    }

    fn review(&self) -> bool {
        !self.options_active()
            && self.form_stage() == Some(crate::tui::sigil_view::app::Stage::Review)
    }

    fn answering(&self) -> bool {
        !self.options_active() && self.draft.peek().is_empty() && self.approval.peek().is_some()
    }

    fn shortcuts(&self, event: &mut KeyEvent) {
        if !self.options_active()
            && !self.view_edit.peek()
            && !self.pane_focus.peek()
            && self.form_stage().is_none()
            && self.approval.peek().is_none()
            && self.draft.peek().is_empty()
        {
            event.stop_propagation();
            self.help.show();
            self.refresh();
        }
    }
}

fn respond() -> Vec<(Shortcut, Handler)> {
    vec![
        (LogicalKey::character("?").into(), App::shortcuts),
        (
            Shortcut::new(LogicalKey::character("?"), ModifiersState::SHIFT),
            App::shortcuts,
        ),
        (
            Shortcut::new(LogicalKey::character("y"), ModifiersState::SHIFT),
            |app, event| {
                if app.answering() && app.approval.peek().is_some_and(|(_, persist)| persist) {
                    event.stop_propagation();
                    app.approval_answer(crate::runtime::GrantAnswer::Always);
                }
            },
        ),
        (LogicalKey::character("y").into(), |app, event| {
            if app.review() {
                event.stop_propagation();
                app.form_reply(crate::tui::sigil_view::app::Reply::Send);
            } else if app.answering() {
                event.stop_propagation();
                app.approval_answer(crate::runtime::GrantAnswer::Allow);
            }
        }),
        (LogicalKey::character("n").into(), |app, event| {
            if app.review() {
                event.stop_propagation();
                app.form_reply(crate::tui::sigil_view::app::Reply::Decline);
            } else if app.answering() {
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
        (Key::Escape.into(), App::escape),
        (Key::Enter.into(), App::enter),
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
    let mut keys: Vec<(Shortcut, Handler)> = vec![
        (LogicalKey::character("e").into(), |app, event| {
            if app.review() {
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
    ];
    keys.extend(hunk_keys());
    keys
}

fn hunk_keys() -> Vec<(Shortcut, Handler)> {
    vec![
        (LogicalKey::character("a").into(), |app, event| {
            if app.pane_active() == Some(super::Pane::Diff) && app.diff_view.peek().is_some() {
                event.stop_propagation();
                app.hunk_accept();
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
    ]
}

impl App {
    fn edge(&self, event: &mut KeyEvent, end: bool) {
        if self.overlay() == Some(Overlay::Usage) {
            return;
        }
        if self.pane_active().is_some() {
            event.stop_propagation();
            self.pane_edge(end);
        } else if self.overlay().is_none() && self.draft.peek().is_empty() {
            event.stop_propagation();
            self.history_edge(end);
        }
    }

    fn ctrl_edge(&self, event: &mut KeyEvent, end: bool) {
        if self.overlay().is_none() {
            event.stop_propagation();
            self.history_edge(end);
        }
    }

    fn page(&self, event: &mut KeyEvent, dir: isize) {
        if self.overlay() == Some(Overlay::Usage) {
            return;
        }
        event.stop_propagation();
        if self.overlay_move(8 * dir) {
            return;
        }
        if self.pane_active().is_some() {
            self.pane_page(dir);
        } else {
            self.page_history(dir as f32);
        }
    }

    fn arrow(&self, event: &mut KeyEvent, dir: isize) {
        if self.overlay() == Some(Overlay::Usage) {
            return;
        }
        if self.overlay_move(dir) {
            event.stop_propagation();
        } else if self.context.peek().is_some() {
            event.stop_propagation();
            self.context_move(dir);
        } else if self.pane_active().is_some() {
            event.stop_propagation();
            self.pane_move(dir);
        } else if dir < 0 && self.draft.peek().is_empty() && self.model.get().queued != (0, 0) {
            event.stop_propagation();
            self.dequeue();
        } else if self.suggesting() {
            event.stop_propagation();
            self.shift(dir);
        }
    }

    fn adjust(&self, event: &mut KeyEvent, dir: isize) {
        if self.overlay_adjust(dir) {
            event.stop_propagation();
        }
    }
}

fn nav() -> Vec<(Shortcut, Handler)> {
    vec![
        (Key::Home.into(), |app, event| app.edge(event, false)),
        (Key::End.into(), |app, event| app.edge(event, true)),
        (Shortcut::ctrl(Key::Home), |app, event| {
            app.ctrl_edge(event, false)
        }),
        (Shortcut::ctrl(Key::End), |app, event| {
            app.ctrl_edge(event, true)
        }),
        (Key::PageUp.into(), |app, event| app.page(event, -1)),
        (Key::PageDown.into(), |app, event| app.page(event, 1)),
        (Key::ArrowUp.into(), |app, event| app.arrow(event, -1)),
        (Key::ArrowDown.into(), |app, event| app.arrow(event, 1)),
        (Key::ArrowLeft.into(), |app, event| app.adjust(event, -1)),
        (Key::ArrowRight.into(), |app, event| app.adjust(event, 1)),
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
    let owner = app.clone();
    let shell = shell
        .key_capture_any(move |event| {
            let typed = event
                .text
                .as_ref()
                .is_some_and(|text| !text.is_empty() && !text.chars().any(char::is_control))
                && !event.modifiers.intersects(
                    ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::META,
                );
            if typed && owner.pane_active().is_some() {
                event.stop_propagation();
            }
        })
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
