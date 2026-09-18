use super::super::{Cmd, Msg};
use super::app::{App, Overlay};
use super::chat as chat_surface;
use super::scene;
use super::settings;
use sigil::prelude::{
    Key, KeyEvent, LayoutBuilderExt, LayoutStyle, LogicalKey, ModifiersState, NodeBuilder,
    NodeEventsExt, NodeStyleExt, Position, Rgba8, Role, ScrollDelta, Shortcut, Style, Theme,
    ThemeValue, WidgetLayoutExt, container, text_edit,
};

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
        let view = match screen.overlay() {
            Some(Overlay::Help) => super::help::layer(screen.clone()),
            Some(Overlay::Search) => super::search::layer(screen.clone()),
            Some(Overlay::Sessions) => super::sessions::layer(screen.clone()),
            Some(Overlay::Usage) => super::usage::layer(screen.clone()),
            Some(Overlay::Auth) => settings::auth_layer(screen.clone()),
            Some(Overlay::Model) => settings::model_picker_layer(screen.clone()),
            Some(Overlay::Workflow) => settings::workflow_layer(screen.clone()),
            Some(Overlay::Hotkeys) => settings::hotkeys_layer(screen.clone()),
            Some(Overlay::Settings) => settings::settings_layer(screen.clone()),
            None if screen.splash.get() => splash(screen.clone()),
            Some(Overlay::Context | Overlay::Cache) | None => chat(screen.clone()),
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
        app.secret.get(),
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
        app.context_open.get(),
        app.cache_open.get(),
        app.meter_hover.get(),
    ));
}

pub(super) fn shell(app: &App) -> NodeBuilder {
    type Handler = fn(&App, &mut KeyEvent);
    let mut shell = container()
        .role(Role::Main)
        .label("Summonr agent harness")
        .theme(Theme::dark())
        .layout(LayoutStyle::flex_column().fill_width().fill_height())
        .class("w-full h-full bg-surface");
    let bindings: &[(Shortcut, Handler)] = &[
        (
            Shortcut::new(LogicalKey::character("y"), ModifiersState::SHIFT),
            |app, event| {
                if app.approval.peek().is_some_and(|(_, persist)| persist) {
                    event.stop_propagation();
                    app.approval_answer("always");
                }
            },
        ),
        (LogicalKey::character("y").into(), |app, event| {
            if app.approval.peek().is_some() {
                event.stop_propagation();
                app.approval_answer("y");
            }
        }),
        (LogicalKey::character("n").into(), |app, event| {
            if app.approval.peek().is_some() {
                event.stop_propagation();
                app.approval_answer("n");
            }
        }),
        (Key::F5.into(), |app, event| {
            if !app.options_active() && !app.view_edit.peek() {
                event.stop_propagation();
                app.retry();
            }
        }),
        (Key::Escape.into(), |app, event| {
            event.stop_propagation();
            if app.options_active() {
                escape(app);
            } else if app.edit_stop() {
            } else if app.context.peek().is_some() {
                app.context_close();
            } else if app.pane_back() {
            } else if app.pane_focus.peek() {
                app.focus_pane(false);
            } else {
                escape(app);
            }
        }),
        (LogicalKey::character("e").into(), |app, event| {
            if app.pane_active() == Some(super::pane::Pane::View) {
                event.stop_propagation();
                app.edit_start();
            }
        }),
        (Shortcut::ctrl(LogicalKey::character("s")), |app, event| {
            if app.view_edit.peek() && !app.hotkey_recording.peek() {
                event.stop_propagation();
                app.edit_save();
            }
        }),
        (LogicalKey::character("t").into(), |app, event| {
            if app.pane_active() == Some(super::pane::Pane::Diff) {
                event.stop_propagation();
                app.toggle_recent();
            }
        }),
        (LogicalKey::character("c").into(), |app, event| {
            if app.pane_active() == Some(super::pane::Pane::Diff) {
                event.stop_propagation();
                app.copy_patch();
            }
        }),
        (LogicalKey::character("o").into(), |app, event| {
            if app.pane_active() == Some(super::pane::Pane::View) {
                event.stop_propagation();
                app.view_external();
            }
        }),
        (Key::Home.into(), |app, event| {
            if app.pane_active().is_some() {
                event.stop_propagation();
                app.pane_edge(false);
            } else if app.edge(false, false) {
                event.stop_propagation();
            }
        }),
        (Key::End.into(), |app, event| {
            if app.pane_active().is_some() {
                event.stop_propagation();
                app.pane_edge(true);
            } else if app.edge(true, false) {
                event.stop_propagation();
            }
        }),
        (Key::ArrowLeft.into(), |app, event| {
            app.placement.set(None);
            if app.pane_active().is_some() {
                event.stop_propagation();
                app.pane_collapse();
            }
        }),
        (Key::ArrowRight.into(), |app, event| {
            app.placement.set(None);
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
            if app.overlay() == Some(Overlay::Sessions) {
                event.stop_propagation();
                app.delete_session();
            } else if !app.hotkey_recording.peek()
                && !app.options_active()
                && app.draft.peek().is_empty()
                && app.model.borrow().busy_since.is_none()
            {
                event.stop_propagation();
                app.quit.set(true);
            }
        }),
        (Key::PageUp.into(), |app, event| {
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
        (Shortcut::ctrl(LogicalKey::character("q")), |app, event| {
            event.stop_propagation();
            app.dequeue();
        }),
        (Key::ArrowUp.into(), |app, event| {
            if app.overlay_move(-1) {
                event.stop_propagation();
            } else if app.context.peek().is_some() {
                event.stop_propagation();
                app.context_move(-1);
            } else if app.pane_active().is_some() {
                event.stop_propagation();
                app.pane_move(-1);
            } else if app.suggesting() {
                event.stop_propagation();
                app.shift(-1);
            } else if app.vertical(-1, false) || app.recall(true) {
                event.stop_propagation();
            }
        }),
        (Key::ArrowDown.into(), |app, event| {
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
            } else if app.vertical(1, false) || app.recall(false) {
                event.stop_propagation();
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
    let scroll = app.clone();
    let shell = shell.child(tab_shortcuts(app)).on_scroll(move |event| {
        let lines = match event.delta {
            ScrollDelta::Pixels(point) => point.y / super::canvas::cell().h,
            ScrollDelta::Lines(point) => point.y,
        };
        if scroll.overlay_move(-lines.round() as isize) {
            event.stop_propagation();
            return;
        }
        let cell = super::canvas::cell();
        if !scroll.options_active()
            && !scroll.splash.peek()
            && scroll
                .todo_hit(
                    (event.position.x / cell.w).max(0.0) as usize,
                    (event.position.y / cell.h).max(0.0) as usize,
                )
                .is_some()
        {
            scroll.scroll_todos(-lines.round() as isize);
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
            ScrollDelta::Lines(point) => point.y * super::canvas::cell().h,
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
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .fill_height()
                        .padding_bottom(super::canvas::cell().h)
                        .gap(super::canvas::cell().h),
                )
                .child(
                    container()
                        .layout(LayoutStyle::flex_column().fill_width().grow(1.0))
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
                                        chat_surface::overlay_rows(app.viewport.get().1),
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
                                        .layout(
                                            LayoutStyle::flex_column().fill().min_size(0.0, 0.0),
                                        )
                                        .class("w-full"),
                                ),
                        ),
                )
                .child(chat_surface::footer_bar(surface.clone())),
        )
        .child(chat_surface::layer(surface))
}

fn theme(_name: &str) -> Theme {
    let mut theme = Theme::dark();
    theme.register_token("accent", ThemeValue::Color(super::canvas::palette().accent));
    theme
}
