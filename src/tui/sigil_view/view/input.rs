use super::*;

pub(super) fn composer_keys(mut node: NodeBuilder, app: &App) -> NodeBuilder {
    node = node.key_capture(Shortcut::new(Key::Enter, ModifiersState::SHIFT), |event| {
        event.key = Key::Enter.into();
        event.modifiers = ModifiersState::empty();
        event.text = None;
    });
    for (key, end) in [(Key::Home, false), (Key::End, true)] {
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

pub(super) fn tab_shortcuts(app: &App) -> NodeBuilder {
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
                    if !complete.overlay_tab(1) {
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
                        mode.cycle_effort();
                    }
                }),
        )
}

pub(super) fn hotkey_shortcuts(mut node: NodeBuilder, app: &App) -> NodeBuilder {
    for key in 'a'..='z' {
        let route = app.clone();
        node = node.key_capture(
            Shortcut::ctrl(LogicalKey::character(key.to_string())),
            move |event| {
                if !route.hotkeys.recording.peek()
                    && !route.options_active()
                    && route.model.get().prefs.hotkeys.action(key)
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

pub(super) fn escape(app: &App) {
    if app.overlay_close() {
        return;
    }
    if app.model.get().busy_since.is_some() {
        drop(app.cmd.send(Cmd::Cancel));
    } else if app.dismiss() {
    } else if app.splash.peek() {
        app.splash.set(false);
    } else {
        app.escape_idle();
    }
}
