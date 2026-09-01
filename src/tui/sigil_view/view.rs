use sigil::prelude::*;

use super::super::Cmd;
use super::app::App;
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
        drop((
            screen.transcript.get(),
            screen.status.get(),
            screen.draft.get(),
            screen.caret.get(),
            screen.viewport.get(),
            screen.scroll_y.get(),
            screen.settings_sel.get(),
            screen.hotkeys_open.get(),
            screen.hotkey_sel.get(),
            screen.hotkey_recording.get(),
            screen.hotkey_note.get(),
            screen.auth_open.get(),
            screen.auth_choices.get(),
            screen.auth_sel.get(),
            screen.auth_entering.get(),
            screen.auth_key.get(),
            screen.auth_caret.get(),
            screen.editing_model.get(),
            screen.model_draft.get(),
            screen.model_caret.get(),
            screen.model_choices.get(),
            screen.model_recent.get(),
            screen.model_sel.get(),
            screen.model_loading.get(),
            screen.model_error.get(),
            screen.workflow_open.get(),
            screen.workflow_name.get(),
            screen.workflow_names.get(),
            screen.workflow_seats.get(),
            screen.workflow_sel.get(),
            screen.workflow_naming.get(),
            screen.workflow_draft.get(),
            screen.workflow_caret.get(),
            screen.drag_from.get(),
            screen.drag_over.get(),
            screen.ticks.get(),
            screen.view_edit.get(),
            screen.view_text.get(),
            screen.view_caret.get(),
        ));
        let view = if screen.auth_open.get() {
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

pub(super) fn shell(app: &App) -> NodeBuilder {
    let cancel = app.clone();
    let interrupt = app.clone();
    let eof = app.clone();
    let complete = app.clone();
    let enter = app.clone();
    let page_up = app.clone();
    let page_down = app.clone();
    let setting_up = app.clone();
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
    hotkey_shortcuts(shell, app)
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
            if save.view_edit.peek() {
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
            }
        })
        .key_capture(Key::End, move |event| {
            if end.pane_active().is_some() {
                event.stop_propagation();
                end.pane_edge(true);
            }
        })
        .key_capture(Key::ArrowLeft, move |event| {
            if left.pane_active().is_some() {
                event.stop_propagation();
                left.pane_collapse();
            }
        })
        .key_capture(Key::ArrowRight, move |event| {
            if right.pane_active().is_some() {
                event.stop_propagation();
                right.pane_expand();
            }
        })
        .child(tab_shortcuts(&complete))
        .key_capture(Key::Enter, move |event| {
            event.stop_propagation();
            if enter.context.peek().is_some() {
                enter.context_enter();
            } else if enter.pane_active().is_some() {
                enter.pane_enter();
            } else if enter.auth_open.peek() {
                enter.commit_auth();
            } else if enter.editing_model.peek() {
                enter.commit_model_edit();
            } else if enter.workflow_open.peek() {
                enter.pick_workflow_seat();
            } else if enter.hotkeys_open.peek() {
                enter.edit_hotkey();
            } else if enter.settings_open.peek() {
                enter.adjust_setting(1);
            } else {
                if !enter.accept() {
                    enter.submit();
                }
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
            event.stop_propagation();
            interrupt.interrupt();
        })
        .key_capture(Shortcut::ctrl(LogicalKey::character("d")), move |event| {
            if !eof.auth_open.peek()
                && eof.draft.peek().is_empty()
                && eof.model.borrow().busy_since.is_none()
            {
                event.stop_propagation();
                eof.quit.set(true);
            }
        })
        .key_capture(Key::PageUp, move |event| {
            event.stop_propagation();
            if page_up.pane_active().is_some() {
                page_up.pane_page(-1);
            } else {
                page_up.page_history(-1.0);
            }
        })
        .key_capture(Key::PageDown, move |event| {
            event.stop_propagation();
            if page_down.pane_active().is_some() {
                page_down.pane_page(1);
            } else {
                page_down.page_history(1.0);
            }
        })
        .key_capture(Key::ArrowUp, move |event| {
            if setting_up.context.peek().is_some() {
                event.stop_propagation();
                setting_up.context_move(-1);
            } else if setting_up.pane_active().is_some() {
                event.stop_propagation();
                setting_up.pane_move(-1);
            } else if setting_up.auth_open.peek() {
                event.stop_propagation();
                setting_up.move_auth(-1);
            } else if setting_up.editing_model.peek() {
                event.stop_propagation();
                setting_up.move_model(-1);
            } else if setting_up.workflow_open.peek() {
                event.stop_propagation();
                setting_up.move_workflow(-1);
            } else if setting_up.hotkeys_open.peek() {
                event.stop_propagation();
                setting_up.move_hotkey(-1);
            } else if setting_up.settings_open.peek() {
                event.stop_propagation();
                setting_up.move_setting(-1);
            } else if setting_up.suggesting() {
                event.stop_propagation();
                setting_up.shift(-1);
            }
        })
        .key_capture(Key::ArrowDown, move |event| {
            if setting_down.context.peek().is_some() {
                event.stop_propagation();
                setting_down.context_move(1);
            } else if setting_down.pane_active().is_some() {
                event.stop_propagation();
                setting_down.pane_move(1);
            } else if setting_down.auth_open.peek() {
                event.stop_propagation();
                setting_down.move_auth(1);
            } else if setting_down.editing_model.peek() {
                event.stop_propagation();
                setting_down.move_model(1);
            } else if setting_down.workflow_open.peek() {
                event.stop_propagation();
                setting_down.move_workflow(1);
            } else if setting_down.hotkeys_open.peek() {
                event.stop_propagation();
                setting_down.move_hotkey(1);
            } else if setting_down.settings_open.peek() {
                event.stop_propagation();
                setting_down.move_setting(1);
            } else if setting_down.suggesting() {
                event.stop_propagation();
                setting_down.shift(1);
            }
        })
        .key_capture(Key::ArrowLeft, move |event| {
            if setting_left.workflow_open.peek()
                && !setting_left.workflow_naming.peek()
                && !setting_left.editing_model.peek()
            {
                event.stop_propagation();
                setting_left.cycle_workflow(-1);
            } else if setting_left.settings_open.peek()
                && !setting_left.hotkeys_open.peek()
                && !setting_left.workflow_open.peek()
                && !setting_left.editing_model.peek()
                && !setting_left.auth_open.peek()
            {
                event.stop_propagation();
                setting_left.adjust_setting(-1);
            }
        })
        .key_capture(Key::ArrowRight, move |event| {
            if setting_right.workflow_open.peek()
                && !setting_right.workflow_naming.peek()
                && !setting_right.editing_model.peek()
            {
                event.stop_propagation();
                setting_right.cycle_workflow(1);
            } else if setting_right.settings_open.peek()
                && !setting_right.hotkeys_open.peek()
                && !setting_right.workflow_open.peek()
                && !setting_right.editing_model.peek()
                && !setting_right.auth_open.peek()
            {
                event.stop_propagation();
                setting_right.adjust_setting(1);
            }
        })
        .key_capture(LogicalKey::character("d"), move |event| {
            if seat_clear.workflow_open.peek()
                && !seat_clear.workflow_naming.peek()
                && !seat_clear.editing_model.peek()
            {
                event.stop_propagation();
                seat_clear.clear_workflow_seat();
            }
        })
        .key_capture(LogicalKey::character("s"), move |event| {
            if seat_save.workflow_open.peek()
                && !seat_save.workflow_naming.peek()
                && !seat_save.editing_model.peek()
            {
                event.stop_propagation();
                seat_save.begin_workflow_save();
            }
        })
        .key_capture(LogicalKey::character(" "), move |event| {
            if setting_space.settings_open.peek()
                && !setting_space.hotkeys_open.peek()
                && !setting_space.workflow_open.peek()
            {
                event.stop_propagation();
                setting_space.adjust_setting(1);
            }
        })
        .key_capture(LogicalKey::character("+"), move |event| {
            if setting_increase.settings_open.peek()
                && !setting_increase.hotkeys_open.peek()
                && !setting_increase.workflow_open.peek()
            {
                event.stop_propagation();
                setting_increase.adjust_setting(1);
            }
        })
        .key_capture(LogicalKey::character("-"), move |event| {
            if setting_decrease.settings_open.peek()
                && !setting_decrease.hotkeys_open.peek()
                && !setting_decrease.workflow_open.peek()
            {
                event.stop_propagation();
                setting_decrease.adjust_setting(-1);
            }
        })
        .key_capture(LogicalKey::character("r"), move |event| {
            if hotkey_reset.hotkeys_open.peek() && !hotkey_reset.hotkey_recording.peek() {
                event.stop_propagation();
                hotkey_reset.reset_hotkeys();
            }
        })
        .on_scroll(move |event| {
            let delta = match event.delta {
                ScrollDelta::Pixels(point) => point.y,
                ScrollDelta::Lines(point) => point.y * super::canvas::cell().h * 3.0,
            };
            let before = scroll.scroll_y.peek();
            scroll.scroll_history(delta);
            if scroll.scroll_y.peek() != before {
                event.stop_propagation();
            }
        })
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
                    if complete.auth_open.peek() {
                        complete.move_auth(1);
                    } else if complete.editing_model.peek() {
                        complete.move_model(1);
                    } else if complete.workflow_open.peek() {
                        complete.move_workflow(1);
                    } else if complete.hotkeys_open.peek() {
                        complete.move_hotkey(1);
                    } else if complete.settings_open.peek() {
                        complete.move_setting(1);
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
                    if mode.auth_open.peek() {
                        mode.move_auth(-1);
                    } else if mode.editing_model.peek() {
                        mode.move_model(-1);
                    } else if mode.workflow_open.peek() {
                        mode.move_workflow(-1);
                    } else if mode.hotkeys_open.peek() {
                        mode.move_hotkey(-1);
                    } else if mode.settings_open.peek() {
                        mode.move_setting(-1);
                    } else {
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
                if route.route_hotkey(key) {
                    event.stop_propagation();
                }
            },
        );
    }
    node
}

fn escape(app: &App) {
    if app.hotkey_recording.peek() {
        app.hotkey_recording.set(false);
        app.hotkey_note.set(String::new());
        return;
    }
    if app.hotkeys_open.peek() {
        app.close_hotkeys();
        return;
    }
    if app.auth_open.peek() {
        app.cancel_auth();
        return;
    }
    if app.editing_model.peek() {
        app.cancel_model_edit();
        return;
    }
    if app.workflow_open.peek() {
        app.close_workflow();
        return;
    }
    if app.settings_open.peek() {
        app.settings_open.set(false);
        return;
    }
    if app.model.borrow().busy_since.is_some() {
        drop(app.cmd.send(Cmd::Cancel));
    } else if !app.dismiss() && app.splash.peek() {
        app.splash.set(false);
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
                .caret_position(app.caret)
                .placeholder("")
                .change(move |draft| changed.change(draft))
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
                    text_edit(value)
                        .caret_position(caret)
                        .placeholder("")
                        .change(move |draft| {
                            if !editing {
                                changed.change(draft);
                            }
                        })
                        .submit(move |_| submit.submit())
                        .height(super::canvas::cell().h * 3.0)
                        .class("w-full"),
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

fn theme(name: &str) -> Theme {
    let mut theme = Theme::dark();
    if name == "fantasy" {
        theme.register_token(
            "accent",
            ThemeValue::Color(Rgba8 {
                r: 255,
                g: 208,
                b: 96,
                a: 255,
            }),
        );
    }
    theme
}
