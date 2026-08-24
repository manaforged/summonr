use super::*;

#[test]
fn splash_is_one_compact_terminal_composition() {
    sigil::reactive::Runtime::scope("mage-splash-frame-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = true;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.splash_revealed.set(true);
        app.resize(160, 50);
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 160, 50, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("{frame}");
        }
        let lines: Vec<&str> = frame.lines().collect();
        let row = |needle: &str| {
            lines
                .iter()
                .position(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("`{needle}` missing:\n{frame}"))
        };
        let logo_row = |text: &str| {
            lines
                .iter()
                .position(|line| line.contains(text))
                .unwrap_or_else(|| panic!("logo row `{text}` missing:\n{frame}"))
        };

        let logo_rows: Vec<usize> = crate::splash::logo()
            .iter()
            .map(|line| {
                let fragment = line
                    .split_whitespace()
                    .max_by_key(|part| part.chars().count())
                    .unwrap_or(line);
                logo_row(fragment)
            })
            .collect();
        assert!(
            logo_rows.windows(2).all(|rows| rows[1] == rows[0] + 1),
            "logo rows split:\n{frame}"
        );
        let first_logo = logo_rows.first().copied().unwrap_or(0);
        assert_eq!(
            row("summoner 0.1.0"),
            first_logo + crate::splash::logo().len() + 1
        );
        assert_eq!(row("model    glm-5.3"), row("summoner 0.1.0") + 1);
        assert!(row("╭ ● auto") > row("policy   strict"));
        assert!(!frame.contains("Enter Mage"), "redundant button:\n{frame}");
        assert!(
            !frame.contains("Enter Summoner"),
            "redundant button:\n{frame}"
        );
    });
}

#[test]
fn fantasy_splash_paints_the_scene_palette() {
    let frame = splash_frame("fantasy", 100, 30, "");
    for color in [
        Rgba8 {
            r: 24,
            g: 15,
            b: 46,
            a: 255,
        },
        Rgba8 {
            r: 255,
            g: 214,
            b: 110,
            a: 255,
        },
        Rgba8 {
            r: 126,
            g: 70,
            b: 104,
            a: 255,
        },
        Rgba8 {
            r: 26,
            g: 52,
            b: 38,
            a: 255,
        },
    ] {
        assert!(has_color(&frame, color), "scene color missing: {color:?}");
    }
}

#[test]
fn space_splash_paints_a_star_field() {
    let frame = splash_frame("space", 160, 50, "");
    assert!(
        frame.text().contains('✦')
            && has_color(
                &frame,
                Rgba8 {
                    r: 176,
                    g: 200,
                    b: 240,
                    a: 255,
                }
            ),
        "star field missing:\n{}",
        frame.text()
    );
}

#[test]
fn focused_splash_prompt_keeps_visible_chrome() {
    let frame = splash_frame("fantasy", 100, 30, "").text();
    assert!(
        frame.contains('╭') && frame.contains('╯') && frame.contains("❯ ▌"),
        "prompt chrome missing:\n{frame}"
    );
    let typed = splash_frame("fantasy", 100, 30, "render check").text();
    assert!(
        typed.contains("❯ render check▌"),
        "prompt text missing:\n{typed}"
    );
}

#[test]
fn composer_keeps_the_caret_and_current_line_visible() {
    sigil::reactive::Runtime::scope("mage-composer-draft-viewport-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(40, 12);
        app.replace("this beginning must scroll away while the newest words stay".to_string());
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 40, 12, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let input = frame.lines().nth(9).unwrap_or_default();
        assert!(
            input.ends_with("the newest words stay▌ │"),
            "draft tail missing:\n{frame}"
        );
        assert!(input.contains('…'), "hidden draft was not marked:\n{frame}");
        assert!(
            !input.contains("this beginning"),
            "draft head stayed pinned:\n{frame}"
        );

        app.replace("first line\ncurrent line".to_string());
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 40, 12, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let input = frame.lines().nth(9).unwrap_or_default();
        assert!(
            input.contains("↵ current line▌"),
            "current line missing:\n{frame}"
        );
        assert!(
            !input.contains("first line"),
            "previous line leaked:\n{frame}"
        );
    });
}

#[test]
fn busy_composer_names_steer_queue_and_cancel() {
    sigil::reactive::Runtime::scope("mage-busy-composer-hints-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.model.borrow_mut().busy_since = Some(Instant::now());
        app.resize(80, 24);
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 80, 24, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("busy 80x24:\n{frame}");
        }
        assert!(
            frame.contains("enter steer · tab queue"),
            "busy actions missing:\n{frame}"
        );
        assert!(
            frame.contains("esc cancel"),
            "cancel action missing:\n{frame}"
        );
        assert!(
            !frame.contains("enter queue"),
            "incorrect action remains:\n{frame}"
        );
    });
}

#[test]
fn compact_surfaces_keep_selected_content_inside_the_frame() {
    sigil::reactive::Runtime::scope("mage-compact-surface-test", || {
        for (width, height) in [(12, 5), (20, 8), (28, 10), (40, 12)] {
            let (cmd, _rx) = unbounded_channel();
            let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
            app.resize(width, height);
            app.replace("/".to_string());
            app.refresh();
            let tree = app.clone();
            sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), width, height, &[])
                .unwrap_or_else(|error| panic!("{width}x{height} chat failed: {error}"));

            app.settings_open.set(true);
            app.settings_sel.set(10);
            let tree = app.clone();
            let frame =
                sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), width, height, &[])
                    .unwrap_or_else(|error| panic!("{width}x{height} settings failed: {error}"));
            if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
                println!("settings {width}x{height}:\n{frame}");
            }
            if width >= 20 {
                assert!(
                    frame.contains("› bus"),
                    "selection scrolled offscreen:\n{frame}"
                );
                let footer = frame.lines().last().unwrap_or_default();
                assert!(
                    footer.contains('╰') && footer.ends_with('╯'),
                    "settings footer escaped its border:\n{frame}"
                );
            }
        }
    });
}

#[test]
fn chat_chrome_keeps_a_horizontal_gutter() {
    sigil::reactive::Runtime::scope("mage-chat-gutter-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.model.borrow_mut().busy_since = Some(Instant::now());
        app.resize(40, 12);
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 40, 12, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let lines: Vec<&str> = frame.lines().collect();
        let composer = lines[8];

        assert!(
            lines[0].contains("summoner glm-5.3") && lines[0].contains("auto"),
            "narrow header hid its primary facts:\n{frame}"
        );
        assert!(
            lines[1].starts_with(' ') && lines[1].trim_start().starts_with("ask anything"),
            "chat content needs left padding:\n{frame}"
        );
        assert!(
            lines[1].contains("ask anything · @file · !cmd · /help"),
            "empty hint was clipped mid-label:\n{frame}"
        );
        assert!(
            composer.ends_with("glm-5.3  ╮") || composer.ends_with("glm-5.3 ╮"),
            "model needs inner right padding:\n{frame}"
        );
        assert!(
            super::chat::SPINNER
                .iter()
                .all(|spinner| !composer.contains(spinner)),
            "composer duplicated the status spinner:\n{frame}"
        );

        for row in [8, 9, 10, 11] {
            assert!(lines[row].starts_with(' '), "left gutter missing:\n{frame}");
            assert_eq!(
                UnicodeWidthStr::width(lines[row]),
                38,
                "right gutter missing:\n{frame}"
            );
        }
    });
}

#[test]
fn narrow_chat_keeps_the_complete_busy_status() {
    sigil::reactive::Runtime::scope("mage-narrow-status-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.workspace = "/home/user/dev/mage".to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        {
            let mut model = app.model.borrow_mut();
            model.busy_since = Some(Instant::now());
            model.rounds = 2;
        }
        app.resize(40, 12);
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 40, 12, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        let status = frame.lines().last().unwrap_or_default();
        assert!(
            status.contains("working · r2 · "),
            "busy status was clipped:\n{frame}"
        );
        assert!(
            status.ends_with('s'),
            "busy elapsed time was clipped:\n{frame}"
        );
    });
}

#[test]
fn narrow_busy_status_keeps_time_ahead_of_tool_detail() {
    sigil::reactive::Runtime::scope("mage-narrow-tool-status-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        {
            let mut model = app.model.borrow_mut();
            model.busy_since = Some(Instant::now());
            model.rounds = 3;
            model.live_tool = Some((
                "run_command".to_string(),
                "a deliberately long command that cannot fit".to_string(),
            ));
        }
        app.resize(40, 12);
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 40, 12, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let status = frame.lines().last().unwrap_or_default();
        assert!(
            status.contains("working · r3 · 0s"),
            "core status missing:\n{frame}"
        );
    });
}

#[test]
fn splash_reveal_and_scene_advance_on_ticks() {
    sigil::reactive::Runtime::scope("mage-splash-animation-parity-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = true;
        start.prefs.theme = "space".to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 30);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 30);
        let initial = screen
            .frame(&mut host, &mut router, [], (100, 30))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(!initial.text().contains("summoner 0.1.0"));
        assert!(!initial.text().contains("❯ "));

        for _ in 0..20 {
            app.tick();
        }
        let animated = screen
            .frame(&mut host, &mut router, [], (100, 30))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(app.splash_revealed.peek());
        assert!(animated.text().contains("summoner 0.1.0"));
        assert!(animated.text().contains("❯ "));
        assert_ne!(initial, animated, "animated scene stayed frozen");
    });
}
