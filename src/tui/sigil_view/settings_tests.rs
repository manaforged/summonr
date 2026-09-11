use super::*;

#[test]
fn autocomplete_and_settings_keep_the_overlays() {
    sigil::reactive::Runtime::scope("mage-overlay-parity-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.refresh();
        app.replace("/".to_string());
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 32);
        let popup = screen
            .frame(&mut host, &mut router, [], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(
            popup.text().contains("› /help"),
            "popup missing:\n{}",
            popup.text()
        );
        assert!(popup.text().contains('╭') && popup.text().contains('╯'));

        app.settings_open.set(true);
        app.settings_sel.set(0);
        let settings = screen
            .frame(&mut host, &mut router, [], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(settings.text().contains("⚙ options"));
        assert!(settings.text().contains("› mode"));
        assert!(settings.text().contains("interface"));
        assert!(
            settings
                .text()
                .contains("↑↓ select · ←→ change · enter open · esc close")
        );
        assert!(has_color(
            &settings,
            Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            }
        ));

        router.focus_next(host.cx());
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::ArrowDown)],
                (100, 32),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.settings_sel.peek(), 1);
        app.settings_sel.set(2);
        let increase = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('+'),
            KeyModifiers::NONE,
        ))
        .unwrap_or_else(|| panic!("+ did not map"));
        screen
            .frame(&mut host, &mut router, [increase], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetEndpoint(_))));
        let typed = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))
        .unwrap_or_else(|| panic!("x did not map"));
        screen
            .frame(&mut host, &mut router, [typed], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.draft.peek(), "/", "settings changed the hidden draft");
    });
}

#[test]
fn options_show_and_rebind_hotkeys() {
    sigil::reactive::Runtime::scope("mage-hotkey-options-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 32);
        screen
            .frame(&mut host, &mut router, [], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        router.focus_next(host.cx());
        let ctrl = |key| {
            sigil_tui::map_key(&TerminalKeyEvent::new(
                KeyCode::Char(key),
                KeyModifiers::CONTROL,
            ))
            .unwrap_or_else(|| panic!("ctrl+{key} did not map"))
        };

        screen
            .frame(&mut host, &mut router, [ctrl('o')], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(app.settings_open.peek());
        app.settings_sel.set(11);
        let options = screen
            .frame(&mut host, &mut router, [], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(options.text().contains("› hotkeys"), "{}", options.text());

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (100, 32),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(app.hotkeys_open.peek());
        let hotkeys = screen
            .frame(&mut host, &mut router, [], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("hotkeys 100x32:\n{}", hotkeys.text());
        }
        assert!(hotkeys.text().contains("› options"), "{}", hotkeys.text());
        assert!(hotkeys.text().contains("ctrl+o"), "{}", hotkeys.text());
        assert!(hotkeys.text().contains("r reset"), "{}", hotkeys.text());

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter), ctrl('c')],
                (100, 32),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(app.hotkey_recording.peek());
        assert_eq!(app.hotkey_note.peek(), "ctrl+c is reserved");
        assert!(!app.quit.peek(), "reserved hotkey triggered ctrl+c");

        screen
            .frame(&mut host, &mut router, [ctrl('k')], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(!app.hotkey_recording.peek());
        assert_eq!(
            app.model
                .borrow()
                .prefs
                .hotkeys
                .label(crate::prefs::Hotkey::Options),
            "ctrl+k"
        );
        let saved = std::fs::read_to_string(
            app.prefs_path
                .as_deref()
                .unwrap_or_else(|| panic!("test has no preference path")),
        )
        .unwrap_or_else(|error| panic!("read saved hotkeys: {error}"));
        assert!(saved.contains("options = \"ctrl+k\""), "{saved}");

        screen
            .frame(&mut host, &mut router, [ctrl('k')], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(!app.hotkeys_open.peek());
        screen
            .frame(&mut host, &mut router, [ctrl('k')], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(!app.settings_open.peek());
        let rebound = screen
            .frame(&mut host, &mut router, [], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(
            rebound.text().contains("enter send · ctrl+k options"),
            "{}",
            rebound.text()
        );
        assert!(!rebound.text().contains("ctrl+o options"));
        let loaded = crate::prefs::load_from(
            app.prefs_path
                .as_deref()
                .unwrap_or_else(|| panic!("test has no preference path")),
        )
        .unwrap_or_else(|error| panic!("reload saved hotkeys: {error}"));
        assert_eq!(
            loaded.hotkeys.label(crate::prefs::Hotkey::Options),
            "ctrl+k"
        );
    });
}

#[test]
fn settings_model_row_opens_the_provider_picker() {
    sigil::reactive::Runtime::scope("mage-settings-model-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.refresh();
        app.settings_open.set(true);
        app.settings_sel.set(3);
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(
            frame.contains("provider"),
            "provider group missing:\n{frame}"
        );
        assert!(frame.contains("endpoint"), "endpoint row missing:\n{frame}");
        assert!(frame.contains("glm-5.3"), "model row missing:\n{frame}");

        app.begin_model_edit();
        assert!(app.editing_model.peek());
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        app.update(Msg::Models {
            recent: 0,
            rows: vec![
                "glm-5.3".to_string(),
                "glm-5.4".to_string(),
                "glm-5.4-air".to_string(),
            ],
        });
        let tree = app.clone();
        let editing = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("model picker 100x32:\n{editing}");
        }
        assert!(
            editing.contains("models"),
            "model picker is not visible:\n{editing}"
        );
        assert!(editing.contains("glm-5.4-air"));
        assert!(editing.contains("type to filter · enter use · esc close"));
        app.move_model(1);
        app.commit_model_edit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetModel(model)) if model == "glm-5.4"));
        assert!(!app.editing_model.peek());
    });
}

#[test]
fn model_picker_sections_recent_then_providers() {
    sigil::reactive::Runtime::scope("mage-model-recent-section-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.begin_model_edit();
        app.update(Msg::Models {
            recent: 1,
            rows: vec![
                "qwen\tqwen3.8-27b".to_string(),
                "glm\tglm-5.3".to_string(),
                "glm\tglm-5.4".to_string(),
                "qwen\tqwen3.8-27b".to_string(),
            ],
        });
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(frame.contains("recent"), "recent header missing:\n{frame}");
        assert!(
            frame.contains("providers"),
            "providers header missing:\n{frame}"
        );
        let recent_at = frame.find("recent").unwrap_or(usize::MAX);
        let providers_at = frame.find("providers").unwrap_or(0);
        assert!(recent_at < providers_at, "recent must sit first:\n{frame}");
        let active: Vec<_> = frame.lines().filter(|line| line.contains('●')).collect();
        assert_eq!(
            active.len(),
            1,
            "only the active model has a marker:\n{frame}"
        );
        assert!(active[0].contains("glm-5.3"));

        app.replace_model_draft("glm".to_string());
        let tree = app.clone();
        let filtered = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(
            !filtered.contains("providers"),
            "filtering must drop the section headers:\n{filtered}"
        );
        assert!(
            filtered.contains("glm-5.3"),
            "filter dropped the match:\n{filtered}"
        );
    });
}

#[test]
fn model_picker_switches_endpoint() {
    sigil::reactive::Runtime::scope("mage-model-switch-endpoint-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.begin_model_edit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        app.update(Msg::Models {
            recent: 0,
            rows: vec![
                "glm\tglm-5.3".to_string(),
                "qwen\tqwen3.8-27b-abliterated".to_string(),
            ],
        });
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(
            frame.contains("qwen") && frame.contains("glm"),
            "picker stayed on one endpoint:\n{frame}"
        );
        app.move_model(1);
        app.commit_model_edit();
        assert!(matches!(
            rx.try_recv(),
            Ok(Cmd::SetSeat(name, model))
                if name == "qwen" && model == "qwen3.8-27b-abliterated"
        ));
    });
}

#[test]
fn auth_command_opens_masked_provider_menu() {
    sigil::reactive::Runtime::scope("mage-auth-menu-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.replace("/auth glm".to_string());
        app.submit();

        assert!(app.auth_open.peek());
        assert!(rx.try_recv().is_err(), "auth command left the UI");
        assert!(app.model.borrow().entries.is_empty());
        let selected = app.auth_sel.peek();
        assert_eq!(app.auth_choices.peek()[selected].name, "glm");
        let tree = app.clone();
        let providers = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 80, 24, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(providers.contains("authentication"));
        assert!(providers.contains("glm"));
        assert!(providers.contains("copilot"));
        assert!(providers.contains("device login"));

        app.commit_auth();
        assert!(app.auth_entering.peek());
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        screen
            .frame(&mut host, &mut router, [], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        router.focus_next(host.cx());
        let frame = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::paste("never-render-this-secret")],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert_eq!(app.auth_key.peek(), "never-render-this-secret");
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("auth menu 80x24:\n{}", frame.text());
        }
        assert!(frame.text().contains("authentication"));
        assert!(frame.text().contains("API key"));
        assert!(frame.text().contains("credential store"));
        assert!(!frame.text().contains("never-render-this-secret"));

        app.commit_auth();
        assert!(matches!(
            rx.try_recv(),
            Ok(Cmd::SetKey(name, key))
                if name == "glm" && key == "never-render-this-secret"
        ));
        assert!(!app.auth_open.peek());
        assert!(app.auth_key.peek().is_empty());
        app.resize(40, 12);
        app.begin_auth(Some("glm"));
        for entering in [false, true] {
            app.auth_entering.set(entering);
            let tree = app.clone();
            let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 40, 12, &[])
                .expect("narrow authentication");
            assert!(
                frame
                    .lines()
                    .nth(10)
                    .expect("footer")
                    .trim_end()
                    .ends_with('╯'),
                "{frame}"
            );
            assert!(frame.contains("esc"), "{frame}");
            if entering {
                assert!(frame.contains("Saves in the credential store"), "{frame}");
                assert!(
                    frame
                        .lines()
                        .nth(9)
                        .expect("storage note")
                        .trim_end()
                        .ends_with('│'),
                    "{frame}"
                );
            }
        }
    });
}

#[test]
fn login_command_uses_device_flow_from_menu() {
    sigil::reactive::Runtime::scope("mage-auth-device-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.splash = true;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.replace("/login copilot".to_string());
        app.submit();

        assert!(app.auth_open.peek());
        app.commit_auth();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Login(name)) if name == "copilot"));
        assert!(!app.auth_open.peek());
        app.replace("keep this draft".to_string());
        app.update(Msg::System("code LOGIN-FIXTURE".to_string()));
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 80, 24, &[])
            .expect("login instructions");
        assert!(frame.contains("code LOGIN-FIXTURE"), "{frame}");
        assert!(frame.contains("keep this draft"), "{frame}");
        assert!(!app.splash.peek());
    });
}

#[test]
fn model_picker_shows_discovery_failures() {
    sigil::reactive::Runtime::scope("mage-model-error-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.begin_model_edit();
        app.update(Msg::ModelsFailed("connection refused".to_string()));
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(
            frame.contains("couldn't load the host list"),
            "missing error state:\n{frame}"
        );
        assert!(
            frame.contains("connection refused"),
            "missing error detail:\n{frame}"
        );
        assert!(
            frame.contains("glm-5.3"),
            "current model missing after a failed list:\n{frame}"
        );
    });
}

#[test]
fn settings_use_group_spacing_in_a_normal_terminal() {
    sigil::reactive::Runtime::scope("mage-settings-spacing-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.refresh();
        app.settings_open.set(true);
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("settings 100x32:\n{frame}");
        }
        let rows: Vec<&str> = frame.lines().collect();
        let session = rows
            .iter()
            .position(|row| row.contains("session"))
            .unwrap_or_else(|| panic!("session group missing:\n{frame}"));
        let provider = rows
            .iter()
            .position(|row| row.contains("provider"))
            .unwrap_or_else(|| panic!("provider group missing:\n{frame}"));
        let interface = rows
            .iter()
            .position(|row| row.contains("interface"))
            .unwrap_or_else(|| panic!("interface group missing:\n{frame}"));

        assert!(
            provider >= session + 5,
            "session group is cramped:\n{frame}"
        );
        assert!(
            interface >= provider + 4,
            "provider group is cramped:\n{frame}"
        );
    });
}

#[test]
fn settings_model_edit_uses_enter_and_escape() {
    sigil::reactive::Runtime::scope("mage-settings-model-keys-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.settings_open.set(true);
        app.settings_sel.set(3);
        app.resize(80, 24);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        screen
            .frame(&mut host, &mut router, [], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        router.focus_next(host.cx());

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(app.editing_model.peek());
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        let typed = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ));
        screen
            .frame(&mut host, &mut router, typed, (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.model_draft.peek(), "x");

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Escape)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(!app.editing_model.peek());
        assert!(rx.try_recv().is_err(), "cancel switched the model");

        app.begin_model_edit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        app.update(Msg::Models {
            recent: 0,
            rows: vec!["glm-5.3".to_string(), "glm-5.4".to_string()],
        });
        app.move_model(1);
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetModel(model)) if model == "glm-5.4"));
    });
}

#[test]
fn settings_tab_moves_between_controls() {
    sigil::reactive::Runtime::scope("mage-settings-tab-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.settings_open.set(true);
        app.resize(80, 24);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        screen
            .frame(&mut host, &mut router, [], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        router.focus_next(host.cx());

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Tab)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.settings_sel.peek(), 1);
        let back = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        ))
        .unwrap_or_else(|| panic!("shift+tab did not map"));
        screen
            .frame(&mut host, &mut router, [back], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.settings_sel.peek(), 0);
        assert!(rx.try_recv().is_err(), "settings tab changed the mode");
    });
}

#[test]
fn the_workflow_card_lists_roles_seats_and_saved_names() {
    sigil::reactive::Runtime::scope("mage-workflow-card-render-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.refresh();
        app.open_workflow();
        apply(
            &app,
            Msg::Workflow {
                name: Some("cheap".to_string()),
                names: vec!["cheap".to_string(), "fast".to_string()],
                seats: vec![None, Some("grok\tgrok-4.6".to_string())],
            },
        );
        app.move_workflow(1);
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(frame.contains("⚙ workflow cheap"), "{frame}");
        assert!(frame.contains("› plan"), "{frame}");
        assert!(frame.contains("grok  grok-4.6"), "{frame}");
        assert!(frame.contains("driving · glm glm-5.3"), "{frame}");
        assert!(frame.contains("saved: [cheap] · fast"), "{frame}");
        assert!(frame.contains("s save"), "{frame}");

        app.begin_workflow_save();
        let tree = app.clone();
        let naming = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(naming.contains("name"), "{naming}");
        assert!(naming.contains("enter save"), "{naming}");

        app.settings_open.set(true);
        app.workflow_open.set(false);
        app.settings_sel.set(4);
        let tree = app.clone();
        let settings = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(settings.contains("› workflow"), "{settings}");
    });
}

fn mouse(kind: crossterm::event::MouseEventKind, col: usize, row: usize) -> sigil_hal::ShellEvent {
    let mut last = sigil_hal::Point::new(0.0, 0.0);
    sigil_tui::map_mouse(
        &crossterm::event::MouseEvent {
            kind,
            column: u16::try_from(col).unwrap_or(u16::MAX),
            row: u16::try_from(row).unwrap_or(u16::MAX),
            modifiers: KeyModifiers::NONE,
        },
        Host::CELL,
        &mut last,
    )
    .unwrap_or_else(|| panic!("mouse report did not map"))
}

#[test]
fn clicks_select_and_act_on_settings_rows() {
    use crossterm::event::{MouseButton, MouseEventKind};
    sigil::reactive::Runtime::scope("mage-settings-click-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.refresh();
        app.settings_open.set(true);
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 32);
        screen
            .frame(&mut host, &mut router, [], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let (col, row) = app
            .hits
            .find(&crate::tui::sigil_view::hit::Target::Setting(2))
            .unwrap_or_else(|| panic!("endpoint row was not painted"));

        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [mouse(kind, col + 3, row)],
                    (100, 32),
                )
                .unwrap_or_else(|error| panic!("layout failed: {error}"));
        }
        assert_eq!(app.settings_sel.peek(), 2, "a click selects the row");
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [mouse(kind, col + 3, row)],
                    (100, 32),
                )
                .unwrap_or_else(|error| panic!("layout failed: {error}"));
        }
        assert!(
            matches!(rx.try_recv(), Ok(Cmd::SetEndpoint(_))),
            "a second click acts on the selected row"
        );
    });
}

#[test]
fn dragging_a_role_row_copies_its_seat_and_clicking_saved_applies() {
    use crossterm::event::{MouseButton, MouseEventKind};
    sigil::reactive::Runtime::scope("mage-workflow-drag-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.refresh();
        app.open_workflow();
        drop(rx.try_recv());
        apply(
            &app,
            Msg::Workflow {
                name: Some("cheap".to_string()),
                names: vec!["cheap".to_string(), "fast".to_string()],
                seats: vec![None, Some("grok\tgrok-4.6".to_string())],
            },
        );
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 32);
        screen
            .frame(&mut host, &mut router, [], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let plan = app
            .hits
            .find(&crate::tui::sigil_view::hit::Target::Role(1))
            .unwrap_or_else(|| panic!("plan row was not painted"));
        let review = app
            .hits
            .find(&crate::tui::sigil_view::hit::Target::Role(3))
            .unwrap_or_else(|| panic!("review row was not painted"));

        screen
            .frame(
                &mut host,
                &mut router,
                [mouse(
                    MouseEventKind::Down(MouseButton::Left),
                    plan.0 + 4,
                    plan.1,
                )],
                (100, 32),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.drag_from.peek(), Some(1));
        let frame = screen
            .frame(
                &mut host,
                &mut router,
                [mouse(
                    MouseEventKind::Drag(MouseButton::Left),
                    review.0 + 4,
                    review.1,
                )],
                (100, 32),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert_eq!(app.drag_over.peek(), Some(3));
        assert!(frame.text().contains("◆ plan"), "{}", frame.text());
        assert!(frame.text().contains("▸ review"), "{}", frame.text());
        assert!(frame.text().contains("drop on a role"), "{}", frame.text());
        screen
            .frame(
                &mut host,
                &mut router,
                [mouse(
                    MouseEventKind::Up(MouseButton::Left),
                    review.0 + 4,
                    review.1,
                )],
                (100, 32),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.drag_from.peek(), None);
        assert!(matches!(
            rx.try_recv(),
            Ok(Cmd::SeatRole(crate::workflow::Role::Review, endpoint, model))
                if endpoint == "grok" && model == "grok-4.6"
        ));
        assert_eq!(app.workflow_sel.peek(), 3);

        let fast = app
            .hits
            .find(&crate::tui::sigil_view::hit::Target::Saved(
                "fast".to_string(),
            ))
            .unwrap_or_else(|| panic!("saved name was not painted"));
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [mouse(kind, fast.0 + 1, fast.1)],
                    (100, 32),
                )
                .unwrap_or_else(|error| panic!("layout failed: {error}"));
        }
        assert!(matches!(rx.try_recv(), Ok(Cmd::Command(line)) if line == "/workflow fast"));

        let scout = app
            .hits
            .find(&crate::tui::sigil_view::hit::Target::Role(0))
            .unwrap_or_else(|| panic!("scout row was not painted"));
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [mouse(kind, scout.0 + 4, scout.1)],
                    (100, 32),
                )
                .unwrap_or_else(|error| panic!("layout failed: {error}"));
        }
        assert!(
            app.editing_model.peek(),
            "a click on a role opens its seat picker"
        );
        assert_eq!(app.model_for.peek(), Some(0));
    });
}
