use super::*;

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

        assert!(app.scroll_y.peek() < bottom);
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
