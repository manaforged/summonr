use sigil::prelude::*;

use super::super::{Cmd, Msg};
use super::app::{App, Overlay};
use super::chat as chat_surface;
use super::scene;
use super::settings;

const SCRIM: Rgba8 = Rgba8 {
    r: 10,
    g: 8,
    b: 20,
    a: 255,
};

pub(super) fn root(app: App) -> NodeBuilder {
    let screen = app.clone();
    shell(&app).children_reactive(move || {
        if let Some(entry) = screen.search.target.peek() {
            match screen.history.borrow_mut().position(&screen, entry) {
                Ok(Some(position)) => screen.scroll_y.set(position),
                Ok(None) => {}
                Err(error) => screen.update(Msg::System(error)),
            }
            screen.search.target.set(None);
        }
        let view = if screen.help.open.get() {
            super::help::layer(screen.clone())
        } else if screen.search.open.get() {
            super::search::layer(screen.clone())
        } else if screen.sessions.open.get() {
            super::sessions::layer(screen.clone())
        } else if screen.auth_open.get() {
            settings::auth_layer(screen.clone())
        } else if screen.editing_model.get() {
            settings::model_picker_layer(screen.clone())
        } else if screen.workflow_open.get() {
            settings::workflow_layer(screen.clone())
        } else if screen.hotkeys_open.get() {
            settings::hotkeys_layer(screen.clone())
        } else if screen.settings_open.get() {
            settings::settings_layer(screen.clone())
        } else if screen.splash.get() {
            splash(screen.clone())
        } else {
            chat(screen.clone())
        };
        vec![
            container()
                .layout(LayoutStyle::block().fill_width().fill_height())
                .child(view)
                .theme(theme(&screen.theme_name.get())),
        ]
    })
}

pub(super) fn watch(app: &App) {
    drop((
        app.help.query.get(),
        app.help.caret.get(),
        app.help.offset.get(),
    ));
    drop((
        app.search.open.get(),
        app.search.query.get(),
        app.search.caret.get(),
        app.search.selected.get(),
    ));
    drop((
        app.panes.get(),
        app.status.get(),
        app.select.get(),
        app.approval.get(),
        app.draft.get(),
        app.caret.get(),
        app.placement.get(),
        app.viewport.get(),
        app.scroll_y.get(),
        app.settings_sel.get(),
        app.hotkeys_open.get(),
        app.hotkey_sel.get(),
        app.hotkey_recording.get(),
        app.hotkey_note.get(),
        app.auth_open.get(),
        app.auth_choices.get(),
        app.auth_sel.get(),
        app.auth_entering.get(),
        app.auth_key.get(),
        app.auth_caret.get(),
        app.editing_model.get(),
        app.model_draft.get(),
        app.model_caret.get(),
        app.model_choices.get(),
        app.model_recent.get(),
        app.model_sel.get(),
        app.model_loading.get(),
        app.model_error.get(),
        app.workflow_open.get(),
        app.workflow_name.get(),
        app.workflow_names.get(),
        app.workflow_seats.get(),
        app.workflow_sel.get(),
        app.workflow_naming.get(),
        app.workflow_draft.get(),
        app.workflow_caret.get(),
        app.drag_from.get(),
        app.drag_over.get(),
        app.ticks.get(),
        app.view_edit.get(),
        app.view_text.get(),
        app.view_caret.get(),
    ));
}

pub(super) fn shell(app: &App) -> NodeBuilder {
    let cancel = app.clone();
    let interrupt = app.clone();
    let eof = app.clone();
    let complete = app.clone();
    let enter = app.clone();
    let retry = app.clone();
    let allow = app.clone();
    let always = app.clone();
    let deny = app.clone();
    let page_up = app.clone();
    let page_down = app.clone();
    let setting_up = app.clone();
    let dequeue = app.clone();
    let setting_down = app.clone();
    let setting_left = app.clone();
    let setting_right = app.clone();
    let setting_space = app.clone();
    let setting_increase = app.clone();
    let setting_decrease = app.clone();
    let seat_clear = app.clone();
    let seat_save = app.clone();
    let hotkey_reset = app.clone();
    let scroll = app.clone();
    let split_left = app.clone();
    let focus_chat = app.clone();
    let focus_pane = app.clone();
    let open_view = app.clone();
    let recent = app.clone();
    let edit = app.clone();
    let save = app.clone();
    let copy = app.clone();
    let home = app.clone();
    let end = app.clone();
    let left = app.clone();
    let right = app.clone();
    let split_right = app.clone();
    let shell = container()
        .role(Role::Main)
        .label("Summoner agent harness")
        .theme(Theme::dark())
        .layout(LayoutStyle::flex_column().fill_width().fill_height())
        .class("w-full h-full bg-surface");
    let shell = shell
        .key_capture(
            Shortcut::new(LogicalKey::character("y"), ModifiersState::SHIFT),
            move |event| {
                if always.approval.peek().is_some_and(|(_, persist)| persist) {
                    event.stop_propagation();
                    always.approval_answer("always");
                }
            },
        )
        .key_capture(LogicalKey::character("y"), move |event| {
            if allow.approval.peek().is_some() {
                event.stop_propagation();
                allow.approval_answer("y");
            }
        })
        .key_capture(LogicalKey::character("n"), move |event| {
            if deny.approval.peek().is_some() {
                event.stop_propagation();
                deny.approval_answer("n");
            }
        })
        .key_capture(Key::F5, move |event| {
            if !retry.options_active() && !retry.view_edit.peek() {
                event.stop_propagation();
                retry.retry();
            }
        })
        .key_capture(Key::Escape, move |event| {
            event.stop_propagation();
            if cancel.options_active() {
                escape(&cancel);
            } else if cancel.edit_stop() {
            } else if cancel.context.peek().is_some() {
                cancel.context_close();
            } else if cancel.pane_back() {
            } else if cancel.pane_focus.peek() {
                cancel.focus_pane(false);
            } else {
                escape(&cancel);
            }
        })
        .key_capture(LogicalKey::character("e"), move |event| {
            if edit.pane_active() == Some(super::pane::Pane::View) {
                event.stop_propagation();
                edit.edit_start();
            }
        })
        .key_capture(Shortcut::ctrl(LogicalKey::character("s")), move |event| {
            if save.view_edit.peek() && !save.hotkey_recording.peek() {
                event.stop_propagation();
                save.edit_save();
            }
        })
        .key_capture(LogicalKey::character("t"), move |event| {
            if recent.pane_active() == Some(super::pane::Pane::Diff) {
                event.stop_propagation();
                recent.toggle_recent();
            }
        })
        .key_capture(LogicalKey::character("c"), move |event| {
            if copy.pane_active() == Some(super::pane::Pane::Diff) {
                event.stop_propagation();
                copy.copy_patch();
            }
        })
        .key_capture(LogicalKey::character("o"), move |event| {
            if open_view.pane_active() == Some(super::pane::Pane::View) {
                event.stop_propagation();
                open_view.view_external();
            }
        })
        .key_capture(Key::Home, move |event| {
            if home.pane_active().is_some() {
                event.stop_propagation();
                home.pane_edge(false);
            } else if home.edge(false, false) {
                event.stop_propagation();
            }
        })
        .key_capture(Key::End, move |event| {
            if end.pane_active().is_some() {
                event.stop_propagation();
                end.pane_edge(true);
            } else if end.edge(true, false) {
                event.stop_propagation();
            }
        })
        .key_capture(Key::ArrowLeft, move |event| {
            left.placement.set(None);
            if left.pane_active().is_some() {
                event.stop_propagation();
                left.pane_collapse();
            }
        })
        .key_capture(Key::ArrowRight, move |event| {
            right.placement.set(None);
            if right.pane_active().is_some() {
                event.stop_propagation();
                right.pane_expand();
            }
        })
        .child(tab_shortcuts(&complete))
        .key_capture(Key::Enter, move |event| {
            if enter.view_edit.peek() && !enter.options_active() {
                return;
            }
            event.stop_propagation();
            if enter.overlay_enter() {
            } else if enter.context.peek().is_some() {
                enter.context_enter();
            } else if enter.pane_active().is_some() {
                enter.pane_enter();
            } else if !enter.accept() {
                enter.submit();
            }
        })
        .key_capture(Shortcut::ctrl(Key::ArrowLeft), move |event| {
            event.stop_propagation();
            focus_chat.focus_pane(false);
        })
        .key_capture(Shortcut::ctrl(Key::ArrowRight), move |event| {
            event.stop_propagation();
            focus_pane.focus_pane(true);
        })
        .key_capture(
            Shortcut::new(
                Key::ArrowLeft,
                ModifiersState::CONTROL | ModifiersState::SHIFT,
            ),
            move |event| {
                event.stop_propagation();
                split_left.nudge_split(-5);
            },
        )
        .key_capture(
            Shortcut::new(
                Key::ArrowRight,
                ModifiersState::CONTROL | ModifiersState::SHIFT,
            ),
            move |event| {
                event.stop_propagation();
                split_right.nudge_split(5);
            },
        )
        .key_capture(Shortcut::ctrl(LogicalKey::character("c")), move |event| {
            if !interrupt.hotkey_recording.peek() {
                event.stop_propagation();
                interrupt.interrupt();
            }
        })
        .key_capture(Shortcut::ctrl(LogicalKey::character("d")), move |event| {
            if eof.overlay() == Some(Overlay::Sessions) {
                event.stop_propagation();
                eof.delete_session();
            } else if !eof.hotkey_recording.peek()
                && !eof.options_active()
                && eof.draft.peek().is_empty()
                && eof.model.borrow().busy_since.is_none()
            {
                event.stop_propagation();
                eof.quit.set(true);
            }
        })
        .key_capture(Key::PageUp, move |event| {
            event.stop_propagation();
            if page_up.overlay_move(-8) {
                return;
            }
            if page_up.pane_active().is_some() {
                page_up.pane_page(-1);
            } else {
                page_up.page_history(-1.0);
            }
        })
        .key_capture(Key::PageDown, move |event| {
            event.stop_propagation();
            if page_down.overlay_move(8) {
                return;
            }
            if page_down.pane_active().is_some() {
                page_down.pane_page(1);
            } else {
                page_down.page_history(1.0);
            }
        })
        .key_capture(Shortcut::ctrl(LogicalKey::character("q")), move |event| {
            event.stop_propagation();
            dequeue.dequeue();
        })
        .key_capture(Key::ArrowUp, move |event| {
            if setting_up.overlay_move(-1) {
                event.stop_propagation();
            } else if setting_up.context.peek().is_some() {
                event.stop_propagation();
                setting_up.context_move(-1);
            } else if setting_up.pane_active().is_some() {
                event.stop_propagation();
                setting_up.pane_move(-1);
            } else if setting_up.suggesting() {
                event.stop_propagation();
                setting_up.shift(-1);
            } else if setting_up.vertical(-1, false) || setting_up.recall(true) {
                event.stop_propagation();
            }
        })
        .key_capture(Key::ArrowDown, move |event| {
            if setting_down.overlay_move(1) {
                event.stop_propagation();
            } else if setting_down.context.peek().is_some() {
                event.stop_propagation();
                setting_down.context_move(1);
            } else if setting_down.pane_active().is_some() {
                event.stop_propagation();
                setting_down.pane_move(1);
            } else if setting_down.suggesting() {
                event.stop_propagation();
                setting_down.shift(1);
            } else if setting_down.vertical(1, false) || setting_down.recall(false) {
                event.stop_propagation();
            }
        })
        .key_capture(Key::ArrowLeft, move |event| {
            if setting_left.overlay_adjust(-1) {
                event.stop_propagation();
            }
        })
        .key_capture(Key::ArrowRight, move |event| {
            if setting_right.overlay_adjust(1) {
                event.stop_propagation();
            }
        })
        .key_capture(LogicalKey::character("d"), move |event| {
            if seat_clear.workflow_keys() {
                event.stop_propagation();
                seat_clear.clear_workflow_seat();
            }
        })
        .key_capture(LogicalKey::character("s"), move |event| {
            if seat_save.workflow_keys() {
                event.stop_propagation();
                seat_save.begin_workflow_save();
            }
        })
        .key_capture(LogicalKey::character(" "), move |event| {
            if setting_space.overlay() == Some(Overlay::Settings) {
                event.stop_propagation();
                setting_space.adjust_setting();
            }
        })
        .key_capture(LogicalKey::character("+"), move |event| {
            if setting_increase.overlay() == Some(Overlay::Settings) {
                event.stop_propagation();
                setting_increase.overlay_adjust(1);
            }
        })
        .key_capture(LogicalKey::character("-"), move |event| {
            if setting_decrease.overlay() == Some(Overlay::Settings) {
                event.stop_propagation();
                setting_decrease.overlay_adjust(-1);
            }
        })
        .key_capture(LogicalKey::character("r"), move |event| {
            if hotkey_reset.overlay() == Some(Overlay::Hotkeys)
                && !hotkey_reset.hotkey_recording.peek()
            {
                event.stop_propagation();
                hotkey_reset.reset_hotkeys();
            }
        })
        .on_scroll(move |event| {
            let lines = match event.delta {
                ScrollDelta::Pixels(point) => point.y / super::canvas::cell().h,
                ScrollDelta::Lines(point) => point.y,
            };
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
            let delta = match event.delta {
                ScrollDelta::Pixels(point) => point.y,
                ScrollDelta::Lines(point) => point.y * super::canvas::cell().h * 3.0,
            };
            let before = scroll.scroll_y.peek();
            scroll.scroll_history(delta);
            if scroll.scroll_y.peek() != before {
                event.stop_propagation();
            }
        });
    hotkey_shortcuts(composer_keys(shell, app), app)
}

fn composer_keys(mut node: NodeBuilder, app: &App) -> NodeBuilder {
    for (key, delta) in [(Key::ArrowUp, -1), (Key::ArrowDown, 1)] {
        let app = app.clone();
        node = node.key_capture(Shortcut::new(key, ModifiersState::SHIFT), move |event| {
            if app.vertical(delta, true) {
                event.stop_propagation();
            }
        });
    }
    for (key, end) in [(Key::Home, false), (Key::End, true)] {
        let select = app.clone();
        node = node.key_capture(Shortcut::new(key, ModifiersState::SHIFT), move |event| {
            if select.edge(end, true) {
                event.stop_propagation();
            }
        });
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

fn tab_shortcuts(app: &App) -> NodeBuilder {
    let complete = app.clone();
    let mode = app.clone();
    let tab = Shortcut::new(Key::Tab, ModifiersState::empty());
    let shift_tab = Shortcut::new(Key::Tab, ModifiersState::SHIFT);
    container()
        .layout(
            LayoutStyle::block()
                .position(Position::Absolute)
                .width(0.0)
                .height(0.0),
        )
        .hittable(false)
        .child(
            container()
                .keyboard_shortcut(tab.clone())
                .key(tab, move |event| {
                    event.stop_propagation();
                    if complete.overlay_tab(1) {
                    } else if complete.model.borrow().busy_since.is_some() {
                        complete.queue_if_busy();
                    } else {
                        complete.complete();
                    }
                }),
        )
        .child(
            container()
                .keyboard_shortcut(shift_tab.clone())
                .key(shift_tab, move |event| {
                    event.stop_propagation();
                    if !mode.overlay_tab(-1) {
                        mode.toggle_mode();
                    }
                }),
        )
}

fn hotkey_shortcuts(mut node: NodeBuilder, app: &App) -> NodeBuilder {
    for key in 'a'..='z' {
        let route = app.clone();
        node = node.key_capture(
            Shortcut::ctrl(LogicalKey::character(key.to_string())),
            move |event| {
                if !route.hotkey_recording.peek()
                    && !route.options_active()
                    && route.model.borrow().prefs.hotkeys.action(key)
                        == Some(crate::prefs::Hotkey::Newline)
                {
                    event.key = Key::Enter.into();
                    event.modifiers = ModifiersState::empty();
                    event.text = None;
                } else if route.route_hotkey(key) {
                    event.stop_propagation();
                }
            },
        );
    }
    node
}

fn escape(app: &App) {
    if app.overlay_close() {
        return;
    }
    if app.model.borrow().busy_since.is_some() {
        drop(app.cmd.send(Cmd::Cancel));
    } else if app.dismiss() {
    } else if app.splash.peek() {
        app.splash.set(false);
    } else {
        app.escape_idle();
    }
}

fn splash(app: App) -> NodeBuilder {
    let submit = app.clone();
    let changed = app.clone();
    let surface = app.clone();
    container()
        .layout(LayoutStyle::block().fill_width().fill_height())
        .child(
            text_edit(app.draft)
                .state(app.editor.clone())
                .caret_position(app.caret)
                .placeholder("")
                .change(move |draft| {
                    changed.pane_focus.set(false);
                    changed.change(draft);
                })
                .submit(move |_| submit.submit())
                .class("w-full h-full"),
        )
        .child(scene::layer(surface))
}

fn chat(app: App) -> NodeBuilder {
    let (cols, rows) = app.viewport.get();
    let layout = app.layout.get();
    let (chat_cols, side_cols) = layout.split(cols);
    let surface = chat_surface(app.clone(), chat_cols);
    match layout.secondary {
        Some(pane) if side_cols > 0 => super::hit::wire(
            &app,
            container()
                .layout(LayoutStyle::flex_row().fill_width().fill_height().gap(0.0))
                .style(Style::new().bg(SCRIM))
                .child(surface)
                .child(super::panes::body(&app, pane, side_cols + 1, rows)),
        ),
        _ => surface,
    }
}

fn chat_surface(app: App, cols: u16) -> NodeBuilder {
    let submit = app.clone();
    let changed = app.clone();
    let surface = app.clone();
    let editing = app.view_edit.get();
    let (value, caret) = if editing {
        (app.view_text, app.view_caret)
    } else {
        (app.draft, app.caret)
    };
    let editor = text_edit(value).caret_position(caret);
    let editor = if editing {
        editor
    } else {
        editor.state(app.editor.clone())
    };
    container()
        .layout(
            LayoutStyle::block()
                .width(f32::from(cols) * super::canvas::cell().w)
                .fill_height(),
        )
        .style(Style::new().bg(SCRIM))
        .child(
            container()
                .layout(LayoutStyle::flex_column().fill_width().fill_height())
                .child(
                    container()
                        .layout(LayoutStyle::flex_column().fill_width())
                        .class("grow"),
                )
                .child(
                    container()
                        .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0))
                        .style_reactive(move || {
                            let rows = chat_surface::input_rows(
                                &app.draft.get(),
                                cols,
                                app.viewport.get().1,
                            );
                            &chat_surface::INPUT_HEIGHTS[usize::from(rows - 1)]
                        })
                        .child(
                            editor
                                .placeholder("")
                                .change(move |draft| {
                                    if !editing {
                                        changed.pane_focus.set(false);
                                        changed.change(draft);
                                    }
                                })
                                .submit(move |_| submit.submit())
                                .layout(LayoutStyle::flex_column().fill().min_size(0.0, 0.0))
                                .class("w-full"),
                        ),
                )
                .child(
                    container().layout(
                        LayoutStyle::flex_row()
                            .fill_width()
                            .height(super::canvas::cell().h),
                    ),
                ),
        )
        .child(chat_surface::layer(surface))
}

fn theme(_name: &str) -> Theme {
    let mut theme = Theme::dark();
    theme.register_token("accent", ThemeValue::Color(super::canvas::palette().accent));
    theme
}
