use super::{State, busy_info, frame_with};

#[test]
fn the_settings_dialog_names_itself_and_groups_advanced_controls() {
    use super::super::{app::App, settings::Group};
    let appearance = |app: &App| {
        app.move_setting(2);
        app.overlay_enter();
        assert!(app.settings_group.peek() == Some(Group::Appearance));
    };
    let back = |app: &App| {
        app.overlay_close();
        assert!(app.settings_open.peek());
        assert!(app.settings_group.peek().is_none());
        assert_eq!(app.settings.cursor.peek(), 2);
    };
    let frames = super::screens(
        110,
        40,
        |app| {
            app.replace("/settings".to_owned());
            app.submit();
        },
        &[&appearance, &back],
    );
    for shown in [&frames[0], &frames[2]] {
        for category in Group::ALL {
            assert!(shown.contains(category.name()), "{shown}");
        }
        assert!(!shown.contains("browser tools"), "{shown}");
        assert!(!shown.contains("run mode"), "{shown}");
    }
    let appearance = &frames[1];
    assert!(appearance.contains("theme"), "{appearance}");
    assert!(appearance.contains("spinner"), "{appearance}");
    assert!(appearance.contains("esc back"), "{appearance}");
    assert!(!appearance.contains("accounts"), "{appearance}");
    println!("Settings home:\n{}\nAppearance:\n{}", frames[0], frames[1]);
}

#[test]
fn the_theme_row_previews_edits_and_saves_custom_colors() {
    let mut seen = Vec::new();
    let shown = frame_with(busy_info(), 110, 40, State::Idle, |app| {
        for _ in 0..summonr_agent::prefs::THEMES.len() {
            app.open_theme();
            app.step_theme(1);
            app.save_theme();
            seen.push(app.model.peek().prefs.theme.clone());
        }
        let original = app.appearance.peek();
        super::super::settings::Setting::Theme.activate(app);
        assert!(app.theme_editor.open.peek());
        app.step_theme(2);
        assert_ne!(app.appearance.peek(), original);
        assert_eq!(
            app.model.peek().prefs.theme,
            summonr_agent::prefs::AUTO_THEME
        );
        app.close_theme();
        assert_eq!(app.appearance.peek(), original);
        app.open_theme();
        app.theme_editor.list.select(1);
        app.edit_theme();
        app.preview_color("éa");
        app.save_theme();
        assert!(
            app.theme_editor.open.peek(),
            "invalid input closed the editor"
        );
        app.preview_color("#102030");
        app.save_theme();
        assert!(!app.theme_editor.open.peek(), "valid colors did not save");
        let saved = std::fs::read_to_string(app.workspace().join(".summonr").join("theme.toml"))
            .expect("saved colors");
        assert!(saved.contains("#102030ff"), "{saved}");
        assert_eq!(
            app.appearance.peek().base,
            sigil::prelude::Color {
                a: 0,
                ..super::super::theme::hex("#102030").expect("color")
            }
        );
        app.open_theme();
    });
    assert_eq!(seen, ["space", "nord", "dawn", "paper", "iron", "auto"]);
    assert!(
        shown.join("\n").contains("Theme editor"),
        "{}",
        shown.join("\n")
    );
}

#[test]
fn theme_keyboard_edits_survive_resize_without_touching_the_draft() {
    use super::super::{app::App, settings::Setting, view::root};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use sigil::{Host, prelude::HostMode, terminal::Screen};
    use summonr_agent::hub::{Activity, Cmd, Fact, Msg};

    sigil::reactive::Owner::scope("theme-input", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(100, 40);
        app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
        app.replace("keep this draft".to_string());
        app.toggle_options();
        app.settings_group.set(Some(Setting::Theme.group()));
        app.settings.select(
            Setting::all()
                .filter(|setting| setting.group() == Setting::Theme.group())
                .position(|setting| setting == Setting::Theme)
                .expect("theme row"),
        );
        let mut host = Host::new(HostMode::Cell);
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 40);
        let key = |code, modifiers| {
            sigil::terminal::map_key(&KeyEvent::new(code, modifiers)).expect("terminal key")
        };
        for _ in 0..3 {
            screen.frame(&mut host, [], (100, 40)).expect("baseline");
        }
        screen
            .frame(
                &mut host,
                [key(KeyCode::Enter, KeyModifiers::NONE)],
                (100, 40),
            )
            .expect("open theme");
        assert!(app.theme_editor.open.peek());
        screen.frame(&mut host, [], (100, 40)).expect("theme frame");
        screen
            .frame(
                &mut host,
                [key(KeyCode::Down, KeyModifiers::NONE)],
                (100, 40),
            )
            .expect("select base");
        screen
            .frame(
                &mut host,
                [key(KeyCode::Enter, KeyModifiers::NONE)],
                (100, 40),
            )
            .expect("edit base");
        for _ in 0..3 {
            screen.frame(&mut host, [], (100, 40)).expect("focus color");
        }
        for _ in 0..9 {
            screen
                .frame(
                    &mut host,
                    [key(KeyCode::Backspace, KeyModifiers::NONE)],
                    (100, 40),
                )
                .expect("clear color");
        }
        let typed = sigil::events::Event::key(
            sigil::events::LogicalKey::character("#102030"),
            sigil::events::KeyPhase::Down,
        )
        .text(Some("#102030".into()));
        screen
            .frame(&mut host, [typed], (100, 40))
            .expect("type color");
        assert_eq!(
            app.appearance.peek().base,
            sigil::prelude::Color {
                a: 0,
                ..super::super::theme::hex("#102030").expect("color")
            }
        );
        for size in [(60, 24), (100, 40)] {
            app.resize(size.0, size.1);
            let mut text = String::new();
            for _ in 0..3 {
                text = screen
                    .frame(&mut host, [], size)
                    .expect("resize editor")
                    .text();
            }
            assert!(text.contains("#102030"), "color lost after resize: {text}");
            assert!(text.contains("apply color"), "editing state lost: {text}");
        }
        for _ in 0..2 {
            screen
                .frame(
                    &mut host,
                    [key(KeyCode::Char('f'), KeyModifiers::NONE)],
                    (100, 40),
                )
                .expect("continue typing after resize");
        }
        screen
            .frame(
                &mut host,
                [key(KeyCode::Enter, KeyModifiers::NONE)],
                (100, 40),
            )
            .expect("apply color");
        screen
            .frame(
                &mut host,
                [key(KeyCode::Char('s'), KeyModifiers::CONTROL)],
                (100, 40),
            )
            .expect("save theme");
        assert!(
            !app.theme_editor.open.peek(),
            "save shortcut did not close editor"
        );
        let file = workspace.path().join(".summonr/theme.toml");
        let saved = std::fs::read(&file).expect("saved theme");
        assert!(
            String::from_utf8(saved.clone())
                .expect("theme text")
                .contains("#102030ff")
        );
        screen
            .frame(&mut host, [], (100, 40))
            .expect("return to settings");
        screen
            .frame(
                &mut host,
                [key(KeyCode::Enter, KeyModifiers::NONE)],
                (100, 40),
            )
            .expect("reopen theme");
        screen
            .frame(
                &mut host,
                [key(KeyCode::Right, KeyModifiers::NONE)],
                (100, 40),
            )
            .expect("preview preset");
        screen
            .frame(
                &mut host,
                [key(KeyCode::Esc, KeyModifiers::NONE)],
                (100, 40),
            )
            .expect("cancel preview");
        assert!(!app.theme_editor.open.peek());
        assert_eq!(std::fs::read(&file).expect("unchanged theme"), saved);
        assert_eq!(
            app.appearance.peek().base,
            sigil::prelude::Color {
                a: 0,
                ..super::super::theme::hex("#102030").expect("restored color")
            }
        );
        assert_eq!(app.draft.peek(), "keep this draft");
        assert!(app.model.peek().busy_since.is_some());
        while let Ok(command) = sent.try_recv() {
            assert!(
                !matches!(command, Cmd::Cancel | Cmd::Submit(_)),
                "editor changed running work"
            );
        }
    });
}

#[test]
fn theme_save_preserves_comments_and_failed_writes_keep_the_editor_open() {
    use super::super::app::App;
    sigil::reactive::Owner::scope("theme-persistence", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let folder = workspace.path().join(".summonr");
        std::fs::create_dir(&folder).expect("theme folder");
        let file = folder.join("theme.toml");
        let original = "# keep this comment\naccent = \"#123456\"\n";
        std::fs::write(&file, original).expect("initial theme");
        let (commands, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.open_theme();
        app.step_theme(1);
        assert_eq!(
            std::fs::read_to_string(&file).expect("preview file"),
            original
        );
        app.close_theme();
        assert_eq!(
            std::fs::read_to_string(&file).expect("cancelled file"),
            original
        );
        app.open_theme();
        app.save_theme();
        let saved = std::fs::read_to_string(&file).expect("saved file");
        assert!(saved.starts_with("# keep this comment"), "{saved}");
        app.toggle_background();
        assert_eq!(
            app.appearance.peek().base.a,
            255,
            "terminal transparency was saved as a theme color"
        );
        let malformed = "base = [";
        std::fs::write(&file, malformed).expect("malformed theme fixture");
        app.open_theme();
        app.save_theme();
        assert!(
            app.theme_editor.open.peek(),
            "invalid TOML closed the editor"
        );
        assert_eq!(
            std::fs::read_to_string(&file).expect("preserved invalid file"),
            malformed
        );
        app.close_theme();
        std::fs::remove_file(&file).expect("remove saved fixture");
        std::fs::create_dir(&file).expect("block the save path");
        app.open_theme();
        app.save_theme();
        assert!(
            app.theme_editor.open.peek(),
            "failed save closed the editor"
        );
        assert!(file.is_dir(), "failed save replaced the existing target");
    });
}

#[test]
fn the_notify_row_shows_the_choice_and_cycles_it() {
    let mut seen = Vec::new();
    let mut label = String::new();
    let _ = frame_with(busy_info(), 110, 40, State::Idle, |app| {
        label = super::super::settings::setting_rows(app)
            .into_iter()
            .find(|row| row.label == "notify")
            .map(|row| row.value)
            .unwrap_or_default();
        for _ in 0..3 {
            app.flip(|prefs| prefs.notify = prefs.notify.toggle());
            seen.push(app.model.peek().prefs.notify.name());
        }
    });
    assert_eq!(label, "turn end and approvals");
    assert_eq!(seen, ["off", "turn", "all"]);
}

#[test]
fn the_window_title_is_the_session_title_without_control_characters() {
    use super::super::app::App;
    use summonr_agent::hub::{Fact, Msg};
    sigil::reactive::Owner::scope("title-controls", || {
        let (commands, _sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), std::path::PathBuf::from("/tmp/work"), commands);
        assert_eq!(app.title(), "Summonr");
        app.update(Msg::Fact(Fact::Live(vec![summonr_agent::hub::Live {
            session: "s1".to_owned(),
            title: "Fix the\u{7}\u{1b}]0;x status bar\nsecond line".to_owned(),
            workspace: std::path::PathBuf::from("/tmp/work"),
            status: summonr_agent::hub::Status::Idle,
            active: true,
            parent: None,
        }])));
        assert_eq!(app.title(), "Fix the]0;x status bar");
    });
}

#[test]
fn the_turn_notification_names_how_the_turn_ended() {
    use summonr_agent::hub::{Fact, Intent, Msg};
    let mut seen = Vec::new();
    let _ = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        seen.push(app.ending());
        app.update(Msg::Intent(Intent::User("next".to_owned())));
        app.update(Msg::Fact(Fact::Failed("provider timed out".to_owned())));
        seen.push(app.ending());
    });
    assert_eq!(seen, ["turn finished", "turn failed"]);
}

#[test]
fn every_dialog_ends_in_its_key_hints() {
    let hotkeys =
        frame_with(busy_info(), 110, 40, State::Idle, |app| app.open_hotkeys()).join("\n");
    assert!(
        hotkeys.contains("↑↓ move · ↵ rebind · r reset all · esc close"),
        "{hotkeys}"
    );
    let settings = frame_with(busy_info(), 110, 40, State::Idle, |app| {
        app.replace("/settings".to_owned());
        app.submit();
    })
    .join("\n");
    assert!(
        settings.contains("↑↓ move · ↵ open · esc close"),
        "{settings}"
    );
}

#[test]
fn adjusting_the_mode_row_in_auto_starts_a_goal_in_the_composer() {
    let mut seen = (String::new(), true);
    let _ = frame_with(busy_info(), 110, 40, State::Idle, |app| {
        app.toggle_options();
        app.settings_group
            .set(Some(super::super::settings::Group::Work));
        let mode = super::super::settings::setting_rows(app)
            .iter()
            .position(|row| row.label == "run mode")
            .expect("a mode row");
        app.settings.cursor.set(mode);
        app.overlay_adjust(1);
        seen = (app.draft.peek(), app.settings_open.peek());
    });
    assert_eq!(seen, ("/goal ".to_owned(), false));
}
