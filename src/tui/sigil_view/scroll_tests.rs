use super::*;
use crate::session::{Prompt, SessionInfo};
use std::sync::Arc;

#[test]
fn transcript_keeps_the_reading_position_when_rows_rewrap() {
    sigil::reactive::Runtime::scope("transcript-reading-position", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(48, 24);
        app.model.borrow_mut().prefs.reasoning = crate::prefs::Reasoning::Expanded;
        app.model.borrow_mut().entries.push(Entry::Thinking(
            (0..40)
                .map(|index| {
                    format!(
                        "LINE-{index:02} {} tail-{index:02}\n",
                        "reading words ".repeat(4)
                    )
                })
                .collect(),
        ));
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(48, 24);
        screen
            .frame(&mut host, &mut router, [], (48, 24))
            .expect("tail");
        app.scroll_y.set(21.0 * Host::CELL);
        let before = screen
            .frame(&mut host, &mut router, [], (48, 24))
            .expect("read continuation")
            .text();
        assert!(
            before
                .lines()
                .nth(2)
                .expect("first row")
                .contains("tail-10"),
            "{before}"
        );
        app.resize(100, 24);
        let after = screen
            .frame(&mut host, &mut router, [], (100, 24))
            .expect("widen")
            .text();
        let row = after.lines().nth(2).expect("first row");
        assert!(
            row.contains("LINE-10") && row.contains("tail-10"),
            "{after}"
        );
        app.resize(48, 24);
        let again = screen
            .frame(&mut host, &mut router, [], (48, 24))
            .expect("narrow")
            .text();
        assert!(
            again.lines().nth(2).expect("first row").contains("LINE-10"),
            "{again}"
        );
    });
}

#[test]
fn transcript_follows_the_tail_through_resize_and_draft_growth() {
    sigil::reactive::Runtime::scope("transcript-reflow", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(100, 24);
        for index in 0..40 {
            app.model.borrow_mut().entries.push(Entry::Assistant {
                text: format!(
                    "**Reply {index}** {}",
                    "words in a longer response ".repeat(6)
                ),
                done: true,
            });
        }
        app.model
            .borrow_mut()
            .entries
            .push(Entry::System("TAIL MARKER".to_string()));
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 24);
        for width in [100, 48, 100] {
            app.resize(width, 24);
            let frame = screen
                .frame(&mut host, &mut router, [], (width, 24))
                .expect("resize")
                .text();
            assert!(frame.contains("TAIL MARKER"), "{frame}");
        }
        app.replace("keep this paragraph ".repeat(40));
        let frame = screen
            .frame(&mut host, &mut router, [], (100, 24))
            .expect("draft grows")
            .text();
        assert!(frame.contains("TAIL MARKER"), "{frame}");
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.update(Msg::Delta("STREAM MARKER".to_string()));
        app.refresh();
        let frame = screen
            .frame(&mut host, &mut router, [], (100, 24))
            .expect("stream tail")
            .text();
        assert!(frame.contains("STREAM MARKER"), "{frame}");
        app.scroll_history(8.0 * Host::CELL);
        let before = screen
            .frame(&mut host, &mut router, [], (100, 24))
            .expect("read older")
            .text();
        assert!(!before.contains("STREAM MARKER"), "{before}");
        app.update(Msg::Delta(" MORE OUTPUT".to_string()));
        app.refresh();
        let after = screen
            .frame(&mut host, &mut router, [], (100, 24))
            .expect("keep reading")
            .text();
        assert_eq!(before.lines().nth(1), after.lines().nth(1));
        assert!(!after.contains("MORE OUTPUT"), "{after}");
    });
}

#[test]
fn scrolling_projection_updates_when_streamed_text_changes() {
    sigil::reactive::Runtime::scope("summonr-streamed-history", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(80, 24);
        app.update(Msg::Delta("stream beginning".to_string()));
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        let initial = screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("first delta")
            .text();
        assert!(initial.contains("stream beginning"));
        app.update(Msg::Delta(" and continuation".to_string()));
        app.refresh();
        let updated = screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("next delta")
            .text();
        assert!(
            updated.contains("stream beginning and continuation"),
            "{updated}"
        );
    });
}

#[test]
fn long_history_scroll_keeps_the_visible_text() {
    sigil::reactive::Runtime::scope("summonr-long-scroll", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        for index in 0..2000 {
            app.model.borrow_mut().entries.push(Entry::Assistant {
                text: format!(
                    "Reply {index}\n\n{}",
                    "Text with **emphasis** and `code`. ".repeat(20)
                ),
                done: true,
            });
        }
        app.resize(120, 40);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        screen
            .frame(&mut host, &mut router, [], (120, 40))
            .expect("initial frame");
        app.scroll_y.set(0.0);
        let mut frames = Vec::new();
        let started = std::time::Instant::now();
        for _ in 0..20 {
            let grid = screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::wheel(10, 5, -1.0)],
                    (120, 40),
                )
                .expect("scroll frame");
            frames.push(grid.text());
        }
        assert!(frames[0].contains("Reply 1"), "{}", frames[0]);
        assert_ne!(frames[0], frames[19]);
        if let Some(path) = std::env::var_os("SUMMON_SCROLL_CAPTURE") {
            std::fs::write(path, serde_json::to_vec(&frames).expect("capture JSON"))
                .expect("write capture");
            eprintln!("20 long-history frames: {:?}", started.elapsed());
        }
    });
}

#[test]
fn the_first_wheel_notch_moves_away_from_the_visible_tail() {
    sigil::reactive::Runtime::scope("summonr-scroll-tail", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(80, 24);
        for index in 0..100 {
            app.model.borrow_mut().entries.push(Entry::Assistant {
                text: format!("```rust\nlet value = {index};\n```"),
                done: true,
            });
        }
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        let tail = screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("tail")
            .text();
        assert!(tail.contains("let value = 99;"));
        let moved = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::wheel(10, 5, 1.0)],
                (80, 24),
            )
            .expect("first notch")
            .text();
        assert_ne!(
            moved, tail,
            "the first notch must move text, not just an estimated offset"
        );
        assert!(!moved.contains("let value = 99;"));
        let returned = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::wheel(10, 5, -1.0)],
                (80, 24),
            )
            .expect("return to tail")
            .text();
        assert_eq!(returned, tail);
    });
}

#[test]
fn transcript_opens_on_the_newest_output() {
    sigil::reactive::Runtime::scope("mage-transcript-tail-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        for index in 0..40 {
            app.model.borrow_mut().entries.push(Entry::System(format!(
                "transcript row {index} {}",
                "wrapped text ".repeat(12)
            )));
        }
        app.refresh();
        let tree = app.clone();
        let Ok(frame) = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 40, 20, &[])
        else {
            panic!("layout failed");
        };

        assert!(
            frame.contains("transcript row 39"),
            "newest row missing:\n{frame}"
        );
        assert!(
            !frame.contains("transcript row 0"),
            "oldest row still visible:\n{frame}"
        );
    });
}

#[test]
fn page_up_scrolls_history_while_the_composer_is_focused() {
    sigil::reactive::Runtime::scope("mage-transcript-page-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(40, 20);
        app.replace("paragraph ".repeat(30));
        for index in 0..40 {
            app.model.borrow_mut().entries.push(Entry::System(format!(
                "transcript row {index} {}",
                "wrapped text ".repeat(12)
            )));
        }
        app.refresh();
        let first = app.clone();
        if let Err(error) = sigil_tui::present(move |cx: &mut Cx| cx.add(root(first)), 40, 20, &[])
        {
            panic!("layout failed: {error}");
        }
        let bottom = app.scroll_y.peek();
        let tree = app.clone();
        let result = sigil_tui::present(
            move |cx: &mut Cx| cx.add(root(tree)),
            40,
            20,
            &[sigil_tui::key_down(Key::PageUp)],
        );
        if let Err(error) = result {
            panic!("layout failed: {error}");
        }

        assert_eq!(bottom - app.scroll_y.peek(), 7.0 * Host::CELL);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.refresh();
        let before = app.scroll_y.peek();
        let tree = app.clone();
        sigil_tui::present(
            move |cx: &mut Cx| cx.add(root(tree)),
            40,
            20,
            &[sigil_tui::key_down(Key::PageUp)],
        )
        .expect("busy page");
        assert_eq!(before - app.scroll_y.peek(), 6.0 * Host::CELL);
    });
}

#[test]
fn wheel_input_moves_the_open_overlay_without_scrolling_the_chat() {
    for mode in ["conversation", "history", "settings"] {
        sigil::reactive::Runtime::scope("overlay-wheel", || {
            let (cmd, _rx) = unbounded_channel();
            let app = App::new(info(), PathBuf::from("."), cmd);
            for index in 0..40 {
                app.model.borrow_mut().entries.push(Entry::Assistant {
                    text: format!("Reply {index:02}"),
                    done: true,
                });
            }
            app.resize(100, 32);
            app.refresh();
            let tree = app.clone();
            let mut host = Host::cells();
            host.mount(move |cx: &mut Cx| cx.add(root(tree)));
            let mut router = EventRouter::new();
            let mut screen = Screen::new(100, 32);
            screen
                .frame(&mut host, &mut router, [], (100, 32))
                .expect("chat");
            app.scroll_y.set(15.0 * Host::CELL);
            let before = screen
                .frame(&mut host, &mut router, [], (100, 32))
                .expect("reading")
                .text();
            let position = app.scroll_y.peek();
            let settings = mode == "settings";
            match mode {
                "settings" => app.settings_open.set(true),
                "history" => {
                    let history = sigil_tui::map_key(&TerminalKeyEvent::new(
                        KeyCode::Char('r'),
                        KeyModifiers::CONTROL,
                    ))
                    .expect("prompt history shortcut");
                    screen
                        .frame(&mut host, &mut router, [history], (100, 32))
                        .expect("prompt history");
                    let session = Arc::new(SessionInfo {
                        id: "saved-session".to_string(),
                        path: PathBuf::from("saved-session"),
                        cwd: None,
                        model: None,
                        preview: String::new(),
                        name: None,
                        parent: None,
                        depth: 0,
                    });
                    app.update(Msg::Prompts {
                        session: None,
                        result: Ok((
                            (0..40)
                                .map(|turn| Prompt {
                                    session: session.clone(),
                                    turn,
                                    text: format!("Saved prompt {turn:02}"),
                                    images: 0,
                                    closed: true,
                                })
                                .collect(),
                            Vec::new(),
                        )),
                    });
                }
                _ => app.search.start("Reply"),
            }
            screen
                .frame(&mut host, &mut router, [], (100, 32))
                .expect("overlay");
            let start = if settings { 8 } else { 24 };
            let opened = screen
                .frame(
                    &mut host,
                    &mut router,
                    (0..start).map(|_| sigil_tui::key_down(Key::ArrowDown)),
                    (100, 32),
                )
                .expect("select row")
                .text();
            assert_eq!(
                if settings {
                    app.settings_sel.peek()
                } else {
                    app.search.selected.peek()
                },
                start,
                "{mode}: {opened}"
            );
            let mut pointer = Point::new(0.0, 0.0);
            let event = MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 10,
                row: 8,
                modifiers: KeyModifiers::NONE,
            };
            let wheel = sigil_tui::map_mouse(&event, Host::CELL, &mut pointer).expect("wheel");
            let moved = screen
                .frame(&mut host, &mut router, [wheel], (100, 32))
                .expect("move overlay")
                .text();
            assert_eq!(
                if settings {
                    app.settings_sel.peek()
                } else {
                    app.search.selected.peek()
                },
                start - 3,
                "{mode}: {moved}"
            );
            assert_ne!(opened, moved);
            let selected = match mode {
                "settings" => "theme",
                "history" => "Saved prompt 21",
                _ => "Reply 18",
            };
            assert!(
                moved
                    .lines()
                    .any(|line| line.contains('›') && line.contains(selected)),
                "{mode}: {moved}"
            );
            assert_eq!(app.scroll_y.peek(), position);
            let down = MouseEvent {
                kind: MouseEventKind::ScrollDown,
                ..event
            };
            let wheel = sigil_tui::map_mouse(&down, Host::CELL, &mut pointer).expect("wheel down");
            let returned = screen
                .frame(&mut host, &mut router, [wheel], (100, 32))
                .expect("return selection")
                .text();
            assert_eq!(returned, opened, "{mode}");
            assert_eq!(app.scroll_y.peek(), position);
            assert!(app.overlay_close());
            let restored = screen
                .frame(&mut host, &mut router, [], (100, 32))
                .expect("close overlay")
                .text();
            assert_eq!(restored, before);
            let wheel = sigil_tui::map_mouse(&event, Host::CELL, &mut pointer).expect("wheel");
            screen
                .frame(&mut host, &mut router, [wheel], (100, 32))
                .expect("move chat");
            assert!(app.scroll_y.peek() < position);
        });
    }
}

#[test]
fn page_keys_move_settings_without_scrolling_the_chat() {
    sigil::reactive::Runtime::scope("overlay-page", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(100, 32);
        for index in 0..80 {
            app.model
                .borrow_mut()
                .entries
                .push(Entry::System(format!("Reading row {index:02}")));
        }
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 32);
        screen
            .frame(&mut host, &mut router, [], (100, 32))
            .expect("chat");
        app.scroll_y.set(15.0 * Host::CELL);
        let before = screen
            .frame(&mut host, &mut router, [], (100, 32))
            .expect("reading")
            .text();
        let position = app.scroll_y.peek();
        app.settings_open.set(true);
        let opened = screen
            .frame(&mut host, &mut router, [], (100, 32))
            .expect("settings")
            .text();
        let moved = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::PageDown)],
                (100, 32),
            )
            .expect("page settings")
            .text();
        assert_eq!(app.scroll_y.peek(), position);
        assert!(
            moved
                .lines()
                .any(|line| line.contains('›') && line.contains("reasoning")),
            "{moved}"
        );
        let returned = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::PageUp)],
                (100, 32),
            )
            .expect("return settings")
            .text();
        assert_eq!(returned, opened);
        let restored = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Escape)],
                (100, 32),
            )
            .expect("close settings")
            .text();
        assert_eq!(restored, before);
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::PageDown)],
                (100, 32),
            )
            .expect("page chat");
        assert!(app.scroll_y.peek() > position);
    });
}

#[test]
fn mouse_wheel_scrolls_transcript_history() {
    sigil::reactive::Runtime::scope("mage-transcript-wheel-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        for index in 0..40 {
            app.model.borrow_mut().entries.push(Entry::System(format!(
                "transcript row {index} {}",
                "wrapped text ".repeat(12)
            )));
        }
        app.resize(40, 20);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(40, 20);
        screen
            .frame(&mut host, &mut router, [], (40, 20))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let bottom = app.scroll_y.peek();
        let mut pointer = Point::new(0.0, 0.0);
        let wheel = sigil_tui::map_mouse(
            &MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 10,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            Host::CELL,
            &mut pointer,
        )
        .unwrap_or_else(|| panic!("wheel did not map"));
        screen
            .frame(&mut host, &mut router, [wheel], (40, 20))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(app.scroll_y.peek() < bottom);
    });
}

#[test]
fn repeated_page_up_reaches_wrapped_history_start() {
    sigil::reactive::Runtime::scope("mage-transcript-full-scroll-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        for index in 0..40 {
            app.model.borrow_mut().entries.push(Entry::System(format!(
                "transcript row {index} {}",
                "wrapped text ".repeat(12)
            )));
        }
        app.refresh();
        let tree = app.clone();
        let mut host = sigil_tui::Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = sigil_tui::EventRouter::new();
        let mut screen = sigil_tui::Screen::new(40, 20);
        screen
            .frame(&mut host, &mut router, [], (40, 20))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        router.focus_next(host.cx());
        for _ in 0..20 {
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::key_down(Key::PageUp)],
                    (40, 20),
                )
                .unwrap_or_else(|error| panic!("layout failed: {error}"));
        }
        let frame = screen
            .frame(&mut host, &mut router, [], (40, 20))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();

        assert!(
            frame.contains("transcript row 0"),
            "oldest row missing:\n{frame}"
        );
    });
}
