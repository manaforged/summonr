use super::*;
use crate::tui::Round;
use sigil_tui::{CellWidth, cell_width, set_cell_width};

#[test]
fn splash_is_one_compact_terminal_composition() {
    sigil::reactive::Runtime::scope("mage-splash-frame-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.splash = true;
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
        assert_eq!(row("setup    /auth · /model · /help"), row("workdir") + 1);
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
fn composer_preserves_graphemes_and_its_right_border() {
    for mode in [
        CellWidth::Unicode,
        CellWidth::Scalar {
            joiner: 1,
            nonjoiner: 1,
            keycap: 1,
        },
    ] {
        set_cell_width(mode);
        for symbol in ["👍🏽", "e\u{301}", "👩\u{200d}💻", "🇳🇿", "1️⃣", "a\u{200c}"]
        {
            sigil::reactive::Runtime::scope("summoner-composer-graphemes", || {
                let (cmd, _rx) = unbounded_channel();
                let app = App::new(info(), PathBuf::from("."), cmd);
                app.resize(40, 12);
                app.replace(format!("A{symbol}B"));
                app.refresh();
                let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(app)), 40, 12, &[])
                    .expect("composer frame");
                let input = frame.lines().nth(9).expect("input row");
                assert!(input.contains(&format!("A{symbol}B▌")), "{frame}");
                assert!(input.ends_with(" │"), "{frame}");
                assert_eq!(cell_width(input), 38, "{frame}");
                let width = cell_width(symbol) + 3;
                let shown =
                    chat::draft_view(&format!("hidden words {symbol}B"), usize::MAX, width, '▌');
                assert!(shown.ends_with(&format!("{symbol}B▌")), "{shown}");
                assert!(cell_width(shown.as_str()) <= width, "{shown}");
            });
        }
    }
    set_cell_width(CellWidth::Unicode);
}

#[test]
fn composer_keeps_the_caret_and_current_line_visible() {
    assert_eq!(chat::draft_view("abcdefx", 6, 6, '▌'), "…def▌x");
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
            input.contains("current line▌"),
            "current line missing:\n{frame}"
        );
        assert!(
            frame
                .lines()
                .nth(8)
                .is_some_and(|line| line.contains("first line")),
            "previous draft line missing:\n{frame}"
        );
    });
}

#[test]
fn busy_composer_names_steer_queue_and_cancel() {
    sigil::reactive::Runtime::scope("mage-busy-composer-hints-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.model.borrow_mut().busy_since = Some((Activity::Turn, Instant::now()));
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
fn login_and_compaction_name_the_operation_and_queue_input() {
    for (activity, label) in [
        (Activity::Login, "signing in"),
        (Activity::Compact, "compacting"),
    ] {
        for width in [40, 80] {
            sigil::reactive::Runtime::scope("summoner-operation-controls", || {
                let (cmd, mut rx) = unbounded_channel();
                let app = App::new(info(), PathBuf::from("."), cmd);
                app.resize(width, 16);
                app.splash.set(false);
                app.update(Msg::Busy(Some(activity)));
                app.refresh();
                let tree = app.clone();
                let frame =
                    sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), width, 16, &[])
                        .expect("busy operation frame");
                assert!(frame.contains("queue"), "{frame}");
                assert!(frame.contains("esc cancel"), "{frame}");
                assert!(frame.contains(label), "{frame}");
                assert!(!frame.contains("steer"), "{frame}");
                assert!(!app.status.peek().contains(" · r1"));
                app.replace("next task".to_string());
                app.submit();
                assert!(
                    matches!(rx.try_recv(), Ok(Cmd::Queue(input)) if input.text == "next task")
                );
                assert!(
                    app.model
                        .borrow()
                        .transcript()
                        .contains("queued for the next turn")
                );
                app.update(Msg::Busy(None));
                app.refresh();
                let tree = app.clone();
                let idle =
                    sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), width, 16, &[])
                        .expect("idle frame");
                assert!(idle.contains("send"), "{idle}");
                assert!(!idle.contains("esc cancel"), "{idle}");
            });
        }
    }
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
        app.model.borrow_mut().busy_since = Some((Activity::Turn, Instant::now()));
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
            lines[1].contains("ask anything · /auth · @file · /help"),
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
fn busy_footer_keeps_usage_visible() {
    sigil::reactive::Runtime::scope("mage-busy-usage-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.workspace = "/home/user/dev/project".to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        {
            let mut model = app.model.borrow_mut();
            model.busy_since = Some((Activity::Turn, Instant::now()));
            model.rounds = 2;
            model.snapshot.context_window = Some(128_000);
            model.usage.push(Round {
                prompt: 32_000,
                completion: 0,
                reasoning: 0,
                cached: 16_000,
                prefix_hash: String::new(),
                cost: None,
            });
            model.prompt_tokens = 32_000;
            model.cached_tokens = 16_000;
            model.live_tool = Some(("run_command".to_string(), "cargo check".to_string()));
        }
        app.resize(100, 12);
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 12, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        let status = frame.lines().last().unwrap_or_default();
        assert!(
            status.contains("ctx 32.0k / 128.0k · 25% used"),
            "busy footer hid context usage:\n{frame}"
        );
        assert!(
            status.contains("50% cached"),
            "busy footer hid cache usage:\n{frame}"
        );
        assert!(
            !status.contains("working") && !status.contains("run_command"),
            "busy footer duplicated activity:\n{frame}"
        );
        assert!(frame.contains("run_command cargo check"), "{frame}");
    });
}

#[test]
fn idle_status_shows_context_limit_and_percentage_used() {
    sigil::reactive::Runtime::scope("mage-context-status-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        {
            let mut model = app.model.borrow_mut();
            model.snapshot.context_window = Some(262_144);
            model.usage.push(Round {
                prompt: 64_000,
                completion: 1_536,
                reasoning: 8_000,
                cached: 32_000,
                prefix_hash: String::new(),
                cost: None,
            });
            model.prompt_tokens = 64_000;
            model.completion_tokens = 1_536;
            model.cached_tokens = 32_000;
        }
        app.resize(100, 32);
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let status = frame.lines().last().unwrap_or_default();

        assert!(
            status.contains("ctx 65.5k / 262.1k · 25% used"),
            "context meter missing:\n{frame}"
        );
        assert!(
            !status.contains("75%"),
            "remaining capacity shown:\n{frame}"
        );
    });
}

#[test]
fn splash_reveal_and_scene_advance_on_ticks() {
    sigil::reactive::Runtime::scope("mage-splash-animation-parity-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.splash = true;
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

#[test]
fn the_composer_shows_what_the_model_is_doing_while_busy() {
    sigil::reactive::Runtime::scope("mage-composer-status-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.workspace = "/home/user/dev/project".to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(80, 16);
        let composer = |app: &App| {
            app.refresh();
            let tree = app.clone();
            let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 80, 16, &[])
                .unwrap_or_else(|error| panic!("layout failed: {error}"));
            let lines: Vec<&str> = frame.lines().collect();
            let top = lines
                .iter()
                .position(|line| line.contains('╭'))
                .unwrap_or_else(|| panic!("no composer border:\n{frame}"));
            let above = lines[top.saturating_sub(1)].to_string();
            let bottom = lines[top + 2].to_string();
            (above, bottom)
        };
        app.update(Msg::Busy(Some(Activity::Turn)));
        let (above, bottom) = composer(&app);
        assert!(above.contains("thinking ·"), "{above}");
        assert!(bottom.contains("esc cancel"), "{bottom}");
        app.update(Msg::Calling("read_file".to_string()));
        assert!(
            composer(&app).0.contains("read_file"),
            "{}",
            composer(&app).0
        );
        app.update(Msg::Event(Event::ToolResult {
            id: "c1".to_string(),
            success: true,
            mutated: false,
            output: "ok".to_string(),
        }));
        assert!(
            composer(&app).0.contains("thinking ·"),
            "{}",
            composer(&app).0
        );
        app.update(Msg::Busy(None));
        let (above, bottom) = composer(&app);
        assert!(bottom.contains("enter send"), "{bottom}");
        assert!(!above.contains("thinking"), "{above}");
    });
}

#[test]
fn the_composer_counts_queued_input_while_busy() {
    sigil::reactive::Runtime::scope("mage-queue-count-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.workspace = "/home/user/dev/project".to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 16);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.update(Msg::Queued {
            steering: 1,
            queued: 1,
        });
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 16, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let lines: Vec<&str> = frame.lines().collect();
        let top = lines
            .iter()
            .position(|line| line.contains('╭'))
            .unwrap_or_default();
        assert!(lines[top.saturating_sub(1)].contains("2 queued"), "{frame}");
        app.dequeue();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Dequeue)));
    });
}

#[test]
fn the_busy_row_and_status_bar_show_tokens_per_second() {
    sigil::reactive::Runtime::scope("mage-speed-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.workspace = "/home/user/dev/project".to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 16);
        let frame = |app: &App| {
            app.refresh();
            let tree = app.clone();
            sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 16, &[])
                .unwrap_or_else(|error| panic!("layout failed: {error}"))
        };
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.update(Msg::Delta("x".repeat(400)));
        let lines: Vec<String> = frame(&app).lines().map(str::to_string).collect();
        let top = lines
            .iter()
            .position(|line| line.contains('╭'))
            .expect("composer");
        assert!(lines[top - 1].contains("tok/s"), "{}", lines[top - 1]);
        app.update(Msg::Event(Event::Usage {
            prompt_tokens: 10,
            completion_tokens: 100,
            reasoning_tokens: 0,
            cached_tokens: 0,
            millis: Some(1_000),
            prefix_hash: String::new(),
            cost: None,
        }));
        app.update(Msg::Busy(None));
        let last = frame(&app).lines().last().unwrap_or_default().to_string();
        assert!(last.contains("tok/s"), "{last}");
    });
}

#[test]
fn an_image_entry_reserves_rows_and_places_a_picture() {
    sigil::reactive::Runtime::scope("mage-image-rows-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.workspace = "/home/user/dev/project".to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 30);
        let source = sigil::prelude::ImageSource::from_rgba8(80, 40, vec![200; 80 * 40 * 4])
            .expect("source");
        app.model.borrow_mut().entries.push(Entry::Image {
            source,
            label: "shot.png".to_string(),
        });
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 30, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let lines: Vec<&str> = frame.lines().collect();
        let label = lines
            .iter()
            .position(|line| line.contains("shot.png"))
            .expect("label row");
        let history = app.history.borrow();
        let placed = history.placements();
        assert_eq!(placed.len(), 1, "one picture placed");
        assert_eq!(placed[0].size(), (80, 20), "80 px wide at half-row height");
        assert_eq!(placed[0].line(), label + 1 - 1, "rows follow the label");
        assert!(
            lines.len() >= label + 21,
            "twenty rows reserved under the label"
        );
    });
}

#[test]
fn a_show_image_call_adds_the_picture_to_the_transcript() {
    sigil::reactive::Runtime::scope("mage-show-image-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        let dir = tempfile::tempdir().expect("tempdir");
        let picture = dir.path().join("plot.png");
        ::image::RgbaImage::from_pixel(3, 3, ::image::Rgba([1, 2, 3, 255]))
            .save(&picture)
            .expect("write png");
        let arguments = serde_json::json!({ "path": picture }).to_string();
        app.update(Msg::Event(Event::ToolCall {
            id: "c1".to_string(),
            name: "show_image".to_string(),
            arguments,
        }));
        let model = app.model.borrow();
        assert!(
            matches!(model.entries.last(), Some(Entry::Image { label, .. }) if label.ends_with("plot.png")),
            "{:?}",
            model.entries.len()
        );
        drop(model);
        app.update(Msg::Event(Event::Assistant {
            text: format!("See ![the plot]({})", picture.display()),
        }));
        let model = app.model.borrow();
        let pictures = model
            .entries
            .iter()
            .filter(|entry| matches!(entry, Entry::Image { .. }))
            .count();
        assert_eq!(pictures, 2, "a markdown image link shows the file too");
    });
}

#[test]
fn a_mermaid_fence_renders_as_a_diagram() {
    sigil::reactive::Runtime::scope("mage-mermaid-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 40);
        app.update(Msg::Event(Event::Assistant {
            text: "Flow:\n```mermaid\ngraph LR\n    A[Parse] --> B[Paint]\n```\ndone".to_string(),
        }));
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 40, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(
            frame.contains("Parse") && frame.contains("Paint"),
            "{frame}"
        );
        assert!(frame.contains('▶'), "arrow head:\n{frame}");
        assert!(!frame.contains("graph LR"), "source hidden:\n{frame}");
    });
}

#[test]
fn a_tall_picture_shows_its_visible_rows_while_scrolled() {
    sigil::reactive::Runtime::scope("mage-tall-picture-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.workspace = "/home/user/dev/project".to_string();
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(60, 14);
        let source = sigil::prelude::ImageSource::from_rgba8(20, 400, vec![90; 20 * 400 * 4])
            .expect("source");
        app.model.borrow_mut().entries.push(Entry::Image {
            source,
            label: "tall.png".to_string(),
        });
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 60, 14, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let placed = app.history.borrow().placements().len();
        assert_eq!(placed, 1);
        assert_eq!(
            app.history.borrow().placements()[0].size().1,
            24,
            "capped at 24 rows"
        );
        assert!(
            frame.contains('▀') || sigil_tui::protocol().is_some(),
            "a picture taller than the body still shows its visible rows:\n{frame}"
        );
    });
}

#[test]
fn the_view_pane_shows_an_image_file() {
    sigil::reactive::Runtime::scope("mage-view-image-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(120, 30);
        let mut bytes = Vec::new();
        ::image::RgbaImage::from_pixel(40, 20, ::image::Rgba([200, 200, 200, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                ::image::ImageFormat::Png,
            )
            .expect("png");
        app.update(Msg::View {
            path: "shot.png".to_string(),
            file: PathBuf::from("/tmp/mage-view-test/shot.png"),
            rope: sigil_ide_core::buffer::Rope::from_reader(std::io::empty()).expect("rope"),
            added: Vec::new(),
            image: Some(bytes),
        });
        app.pane_command(Some("view"));
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 120, 30, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(frame.contains("view · shot.png · 40×20"), "{frame}");
        assert!(
            frame.contains('▀') || sigil_tui::protocol().is_some(),
            "the pane draws the picture:\n{frame}"
        );
    });
}
