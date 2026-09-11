use super::*;

#[test]
fn saved_prompt_search_inserts_at_the_caret_and_preserves_the_draft_image() {
    for width in [40, 100] {
        sigil::reactive::Runtime::scope("summoner-saved-prompts", || {
            let dir = tempfile::tempdir().expect("workspace");
            let path = dir.path().join("draft.png");
            image::RgbImage::new(1, 1).save(&path).expect("draft image");
            let bytes = std::fs::read(&path).expect("image bytes");
            let (cmd, mut rx) = unbounded_channel();
            let app = App::new(info(), dir.path().to_path_buf(), cmd);
            app.resize(width, 16);
            assert!(app.paste("draft.png"));
            app.replace("before 🦀 after".to_string());
            app.caret.set(7);
            app.refresh();
            let tree = app.clone();
            let mut host = Host::cells();
            host.mount(move |cx: &mut Cx| cx.add(root(tree)));
            let mut router = EventRouter::new();
            let mut screen = Screen::new(width, 16);
            screen
                .frame(&mut host, &mut router, [], (width, 16))
                .expect("composer");
            router.focus_next(host.cx());
            let history = sigil_tui::map_key(&TerminalKeyEvent::new(
                KeyCode::Char('r'),
                KeyModifiers::CONTROL,
            ))
            .expect("ctrl+r");
            let frame = screen
                .frame(&mut host, &mut router, [history], (width, 16))
                .expect("prompt history");
            assert!(
                frame.text().contains("Loading saved prompts"),
                "{}",
                frame.text()
            );
            assert!(matches!(rx.try_recv(), Ok(Cmd::ListPrompts(None))));
            let session = std::sync::Arc::new(crate::session::SessionInfo {
                id: "saved-session".to_string(),
                path: PathBuf::from("saved-session"),
                cwd: Some("/other-project".to_string()),
                model: Some("test-model".to_string()),
                preview: String::new(),
                name: Some("Parser history".to_string()),
                parent: None,
                depth: 0,
            });
            let saved = "Needle [42] 🦀\nfinish ";
            app.update(Msg::Prompts {
                session: None,
                result: Ok((
                    vec![
                        crate::session::Prompt {
                            session: session.clone(),
                            turn: 2,
                            text: saved.to_string(),
                            images: 1,
                            closed: true,
                        },
                        crate::session::Prompt {
                            session: session.clone(),
                            turn: 1,
                            text: "unrelated prompt".to_string(),
                            images: 0,
                            closed: true,
                        },
                        crate::session::Prompt {
                            session,
                            turn: 0,
                            text: saved.to_string(),
                            images: 0,
                            closed: true,
                        },
                    ],
                    vec!["Cannot read session broken-session: invalid record".to_string()],
                )),
            });
            app.refresh();
            assert!(
                app.transcript
                    .peek()
                    .contains("broken-session: invalid record")
            );
            app.update(Msg::Prompts {
                session: Some("old-session".to_string()),
                result: Err("stale branch response".to_string()),
            });
            let frame = screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::paste("needle [42] 🦀")],
                    (width, 16),
                )
                .expect("filtered prompts");
            assert!(frame.text().contains("Parser history"), "{}", frame.text());
            assert!(frame.text().contains("1 skipped"), "{}", frame.text());
            assert_eq!(
                app.search.rows(&app.model.borrow()).expect("matches").len(),
                1
            );
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::key_down(Key::Enter)],
                    (width, 16),
                )
                .expect("insert prompt text");
            assert!(!app.search.open.peek());
            assert_eq!(app.draft.peek(), format!("before {saved}🦀 after"));
            assert_eq!(app.caret.peek(), 7 + saved.len());
            assert!(rx.try_recv().is_err(), "selection must not send a prompt");
            assert!(
                app.model
                    .borrow()
                    .transcript()
                    .contains("Saved images were not added")
            );
            app.submit();
            let input = match rx.try_recv() {
                Ok(Cmd::Submit(input)) => input,
                _ => panic!("submitted prompt"),
            };
            assert_eq!(input.text, format!("before {saved}🦀 after"));
            assert_eq!(
                input.images,
                [crate::provider::Image::encode("image/png", &bytes)]
            );
        });
    }
}

#[test]
fn failed_or_empty_prompt_search_can_close_without_cancelling_an_active_operation() {
    for activity in [Activity::Turn, Activity::Login, Activity::Compact] {
        sigil::reactive::Runtime::scope("summoner-prompt-search-errors", || {
            let (cmd, mut rx) = unbounded_channel();
            let app = App::new(info(), PathBuf::from("."), cmd);
            app.resize(40, 16);
            app.replace("unfinished draft".to_string());
            app.caret.set(4);
            app.update(Msg::Busy(Some(activity)));
            assert!(app.route_hotkey('r'));
            assert!(matches!(rx.try_recv(), Ok(Cmd::ListPrompts(None))));
            let tree = app.clone();
            let mut host = Host::cells();
            host.mount(move |cx: &mut Cx| cx.add(root(tree)));
            let mut router = EventRouter::new();
            let mut screen = Screen::new(40, 16);
            for (result, expected) in [
                (
                    Err("Cannot read saved session".to_string()),
                    "Cannot read saved session",
                ),
                (Ok((Vec::new(), Vec::new())), "No matches"),
            ] {
                app.update(Msg::Prompts {
                    session: None,
                    result,
                });
                let frame = screen
                    .frame(&mut host, &mut router, [], (40, 16))
                    .expect("unavailable prompts");
                assert!(frame.text().contains(expected), "{}", frame.text());
                assert!(frame.text().contains("esc"), "{}", frame.text());
                router.focus_next(host.cx());
                screen
                    .frame(
                        &mut host,
                        &mut router,
                        [sigil_tui::key_down(Key::Enter)],
                        (40, 16),
                    )
                    .expect("empty selection");
                assert!(app.search.open.peek());
            }
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::key_down(Key::Escape)],
                    (40, 16),
                )
                .expect("cancel search");
            assert!(!app.search.open.peek());
            assert_eq!(app.draft.peek(), "unfinished draft");
            assert_eq!(app.caret.peek(), 4);
            assert_eq!(
                app.model.borrow().busy_since.map(|(activity, _)| activity),
                Some(activity)
            );
            assert!(
                rx.try_recv().is_err(),
                "closing search must not cancel the operation"
            );
        });
    }
}

#[test]
fn search_jumps_to_the_selected_message_after_resize_without_changing_the_draft() {
    sigil::reactive::Runtime::scope("summoner-conversation-search", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(100, 24);
        for index in 0..40 {
            let text = if index == 3 || index == 24 {
                format!("message {index} Needle [42] 🦀")
            } else {
                format!("message {index} {}", "wrapped conversation text ".repeat(8))
            };
            app.update(Msg::User(text));
        }
        app.replace("keep my unfinished draft".to_string());
        app.caret.set(7);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 24);
        screen
            .frame(&mut host, &mut router, [], (100, 24))
            .expect("chat");
        router.focus_next(host.cx());
        let find = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('f'),
            KeyModifiers::CONTROL,
        ))
        .expect("ctrl+f");
        let frame = screen
            .frame(&mut host, &mut router, [find], (100, 24))
            .expect("search");
        assert!(frame.text().contains("Search conversation"));
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::paste("needle [42] 🦀")],
                (100, 24),
            )
            .expect("literal Unicode query");
        let rows = app.search.rows(&app.model.borrow()).expect("results");
        assert_eq!(
            rows.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            [24, 3]
        );
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::ArrowDown)],
                (100, 24),
            )
            .expect("older match");
        app.resize(40, 16);
        screen
            .frame(&mut host, &mut router, [], (40, 16))
            .expect("resize search");
        let frame = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (40, 16),
            )
            .expect("jump");
        assert!(
            frame.text().contains("message 3 Needle [42]"),
            "{}",
            frame.text()
        );
        assert!(!app.search.open.peek());
        assert_eq!(app.draft.peek(), "keep my unfinished draft");
        assert_eq!(app.caret.peek(), 7);
        assert!(rx.try_recv().is_err(), "search must not submit or cancel");
        app.replace("continue after finding a message".to_string());
        app.submit();
        let frame = screen
            .frame(&mut host, &mut router, [], (40, 16))
            .expect("continue from the search");
        assert!(
            frame.text().contains("continue after finding a message"),
            "{}",
            frame.text()
        );
        assert!(
            matches!(rx.try_recv(), Ok(Cmd::Submit(input)) if input.text == "continue after finding a message")
        );
    });
}

#[test]
fn empty_search_can_close_during_streaming_without_cancelling_the_turn() {
    sigil::reactive::Runtime::scope("summoner-busy-search", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(40, 16);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.replace("keep the draft".to_string());
        app.search.start("missing text");
        app.update(Msg::Delta("streaming answer".to_string()));
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(40, 16);
        let frame = screen
            .frame(&mut host, &mut router, [], (40, 16))
            .expect("empty search");
        assert!(frame.text().contains("No matches"), "{}", frame.text());
        router.focus_next(host.cx());
        screen
            .frame(
                &mut host,
                &mut router,
                [
                    sigil_tui::key_down(Key::Tab),
                    sigil_tui::key_down(Key::Enter),
                    sigil_tui::key_down(Key::Escape),
                ],
                (40, 16),
            )
            .expect("close search");
        assert!(!app.search.open.peek());
        assert!(app.model.borrow().busy_since.is_some());
        assert_eq!(app.draft.peek(), "keep the draft");
        assert!(rx.try_recv().is_err());
    });
}

#[test]
fn image_paste_errors_preserve_the_draft_and_never_submit_an_attachment() {
    sigil::reactive::Runtime::scope("summoner-image-errors", || {
        let dir = tempfile::tempdir().expect("workspace");
        std::fs::write(dir.path().join("wrong.png"), b"plain text").expect("wrong format");
        std::fs::write(dir.path().join("broken.png"), b"\x89PNG\r\n\x1a\n")
            .expect("truncated image");
        std::fs::File::create(dir.path().join("large.png"))
            .expect("large file")
            .set_len(crate::tools::IMAGES_MAX_BYTES as u64 + 1)
            .expect("large image size");
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), dir.path().to_path_buf(), cmd);
        app.replace("unfinished draft".to_string());
        app.caret.set(4);
        for name in ["missing.png", "wrong.png", "broken.png", "large.png"] {
            assert!(app.paste(name), "{name} must not fall back to text");
            assert_eq!(app.draft.peek(), "unfinished draft");
            assert_eq!(app.caret.peek(), 4);
            assert!(
                matches!(app.model.borrow().entries.last(), Some(Entry::Error(error))
                if error.contains("cannot attach image") && error.contains(name))
            );
        }
        assert!(!app.paste("please inspect missing.png"));
        assert!(!app.paste("ordinary pasted text"));
        assert!(rx.try_recv().is_err());
        app.submit();
        assert!(
            matches!(rx.try_recv(), Ok(Cmd::Submit(input)) if input.images.is_empty()
            && input.text == "unfinished draft")
        );
    });
}

#[test]
fn image_paste_and_replay_render_the_saved_pixels() {
    sigil::reactive::Runtime::scope("summoner-image-replay", || {
        let dir = tempfile::tempdir().expect("workspace");
        let path = dir.path().join("reference.png");
        image::RgbImage::from_pixel(4, 4, image::Rgb([255, 0, 0]))
            .save(&path)
            .expect("image fixture");
        let bytes = std::fs::read(&path).expect("image bytes");
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), dir.path().to_path_buf(), cmd);
        app.resize(80, 20);
        assert!(app.paste("reference.png"));
        match app.model.borrow().entries.last().expect("preview") {
            Entry::Image { source, .. } => {
                assert_eq!((source.width(), source.height()), (4, 4));
                assert_eq!(source.rgba(), [255, 0, 0, 255].repeat(16));
            }
            other => panic!("expected image, got {other:?}"),
        }
        app.replace("describe the image".to_string());
        app.submit();
        let input = match rx.try_recv() {
            Ok(Cmd::Submit(input)) => input,
            _ => panic!("image submission"),
        };
        assert_eq!(
            input.images,
            [crate::provider::Image::encode("image/png", &bytes)]
        );
        app.update(Msg::Cleared);
        app.update(Msg::User(input.text));
        for image in input.images {
            app.update(Msg::Image(image));
        }
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 80, 20, &[])
            .expect("restored image frame");
        assert!(frame.contains("Attached image · image/png"), "{frame}");
        assert!(frame.contains("▀▀▀▀"), "{frame}");
        assert!(!frame.contains("could not render"), "{frame}");
    });
}

#[test]
#[ignore = "requires a native clipboard containing the 2x2 RGBA fixture"]
fn clipboard_image_preserves_draft_pixels_and_input_scope() {
    sigil::reactive::Runtime::scope("summoner-clipboard-image", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(80, 20);
        app.replace("describe this".to_string());
        app.caret.set(4);
        app.auth_open.set(true);
        assert!(!app.clipboard());
        app.auth_open.set(false);
        app.view_edit.set(true);
        assert!(!app.clipboard());
        app.view_edit.set(false);
        assert!(app.clipboard());
        assert_eq!(app.draft.peek(), "describe this");
        assert_eq!(app.caret.peek(), 4);
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 80, 20, &[])
            .expect("clipboard image frame");
        assert!(frame.contains("Clipboard image"), "{frame}");
        assert!(frame.contains("▀▀"), "{frame}");
        app.submit();
        let input = match rx.try_recv().expect("submission") {
            Cmd::Submit(input) => input,
            _ => panic!("expected submit"),
        };
        assert_eq!(input.text, "describe this");
        assert_eq!(input.images.len(), 1);
        assert_eq!(input.images[0].media_type, "image/png");
        let rgba = image::load_from_memory(&input.images[0].bytes)
            .expect("submitted PNG")
            .into_rgba8();
        assert_eq!(rgba.dimensions(), (2, 2));
        assert_eq!(
            rgba.as_raw(),
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255
            ]
        );
    });
}

#[test]
fn fork_updates_the_header_and_keeps_the_parent_available_for_resume() {
    sigil::reactive::Runtime::scope("summoner-fork-identity", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        let parent = app.info.session.clone();
        let child = "child-abcde";
        app.resize(100, 16);
        app.splash.set(false);
        app.update(Msg::Session(child.to_string()));
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 16, &[])
            .expect("fork header");
        assert!(
            frame.lines().next().expect("header").contains("ses abcde"),
            "{frame}"
        );
        assert!(app.header.peek().contains("session abcde"));
        let session = |id: &str, name: &str| crate::session::SessionInfo {
            id: id.to_string(),
            name: Some(name.to_string()),
            cwd: Some(".".to_string()),
            model: Some("test".to_string()),
            preview: String::new(),
            path: PathBuf::from(id),
            parent: None,
            depth: 0,
        };
        app.sessions.start(&app.model.borrow().session, false);
        app.update(Msg::Sessions(Ok(vec![
            (session(&parent, "Parent task"), true),
            (session(child, "Current branch"), true),
        ])));
        let rows = app.sessions.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, parent);
    });
}

#[test]
fn retry_keeps_the_draft_and_refuses_an_active_turn() {
    sigil::reactive::Runtime::scope("summoner-retry", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(80, 16);
        app.replace("an unrelated draft".to_string());
        app.caret.set(4);
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 16);
        screen
            .frame(&mut host, &mut router, [], (80, 16))
            .expect("frame");
        router.focus_next(host.cx());
        screen
            .frame(
                &mut host,
                &mut router,
                [
                    sigil_tui::map_key(&TerminalKeyEvent::new(KeyCode::F(5), KeyModifiers::NONE))
                        .expect("terminal F5"),
                ],
                (80, 16),
            )
            .expect("retry");
        assert!(matches!(rx.try_recv(), Ok(Cmd::Command(line)) if line == "/retry"));
        assert_eq!(app.draft.peek(), "an unrelated draft");
        assert_eq!(app.caret.peek(), 4);
        app.update(Msg::Busy(Some(Activity::Turn)));
        screen
            .frame(
                &mut host,
                &mut router,
                [
                    sigil_tui::map_key(&TerminalKeyEvent::new(KeyCode::F(5), KeyModifiers::NONE))
                        .expect("terminal F5"),
                ],
                (80, 16),
            )
            .expect("busy retry");
        assert!(rx.try_recv().is_err());
        assert_eq!(app.draft.peek(), "an unrelated draft");
        assert!(
            app.model
                .borrow()
                .transcript()
                .contains("Cancel the active turn before retrying")
        );
    });
}

#[test]
fn returned_prompts_preserve_the_current_draft_and_pasted_images() {
    sigil::reactive::Runtime::scope("summoner-pending-input-recovery", || {
        let dir = tempfile::tempdir().expect("workspace");
        let path = dir.path().join("pasted.png");
        image::RgbImage::new(1, 1).save(&path).expect("draft image");
        let bytes = std::fs::read(&path).expect("image bytes");
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), dir.path().to_path_buf(), cmd);
        assert!(app.paste(&path.to_string_lossy()));
        app.replace("unfinished draft".to_string());
        app.caret.set(5);
        let mut first: crate::provider::UserInput = "first prompt".into();
        first
            .images
            .push(crate::provider::Image::encode("image/png", b"pasted image"));
        app.update(Msg::Restored(vec![first, "second prompt".into()]));
        assert_eq!(
            app.draft.peek(),
            "first prompt\n\nsecond prompt\n\nunfinished draft"
        );
        assert_eq!(
            app.caret.peek(),
            "first prompt\n\nsecond prompt\n\n".len() + 5
        );
        assert!(rx.try_recv().is_err(), "restoration must not submit");
        app.submit();
        let input = match rx.try_recv() {
            Ok(Cmd::Submit(input)) => input,
            _ => panic!("submitted input"),
        };
        assert_eq!(
            input.text,
            "first prompt\n\nsecond prompt\n\nunfinished draft"
        );
        assert_eq!(
            input.images,
            [
                crate::provider::Image::encode("image/png", b"pasted image"),
                crate::provider::Image::encode("image/png", &bytes)
            ]
        );
    });
}

#[test]
fn branching_selects_a_closed_turn_in_the_current_session_and_preserves_the_draft() {
    for width in [40, 100] {
        sigil::reactive::Runtime::scope("summoner-branch-picker", || {
            let (cmd, mut rx) = unbounded_channel();
            let app = App::new(info(), PathBuf::from("."), cmd);
            app.resize(width, 16);
            app.replace("/branch".to_string());
            app.submit();
            assert!(
                matches!(rx.try_recv(), Ok(Cmd::ListPrompts(Some(id))) if id == app.model.borrow().session)
            );
            app.replace("keep this draft".to_string());
            app.caret.set(4);
            let current = app.model.borrow().session.clone();
            let session = |id: &str| {
                std::sync::Arc::new(crate::session::SessionInfo {
                    id: id.to_string(),
                    path: PathBuf::from(id),
                    cwd: None,
                    model: None,
                    preview: String::new(),
                    name: None,
                    parent: None,
                    depth: 0,
                })
            };
            app.update(Msg::Prompts {
                session: Some(current.clone()),
                result: Ok((
                    vec![
                        crate::session::Prompt {
                            session: session(&current),
                            turn: 2,
                            text: "open turn".to_string(),
                            images: 0,
                            closed: false,
                        },
                        crate::session::Prompt {
                            session: session("other"),
                            turn: 0,
                            text: "other session".to_string(),
                            images: 0,
                            closed: true,
                        },
                        crate::session::Prompt {
                            session: session(&current),
                            turn: 1,
                            text: "repeated prompt".to_string(),
                            images: 0,
                            closed: true,
                        },
                        crate::session::Prompt {
                            session: session(&current),
                            turn: 0,
                            text: "repeated prompt".to_string(),
                            images: 1,
                            closed: true,
                        },
                    ],
                    Vec::new(),
                )),
            });
            assert_eq!(
                app.search
                    .rows(&app.model.borrow())
                    .expect("branch choices")
                    .len(),
                2
            );
            app.update(Msg::Prompts {
                session: None,
                result: Err("stale history response".to_string()),
            });
            assert_eq!(
                app.search
                    .rows(&app.model.borrow())
                    .expect("branch ignores stale history")
                    .len(),
                2
            );
            let tree = app.clone();
            let mut host = Host::cells();
            host.mount(move |cx: &mut Cx| cx.add(root(tree)));
            let mut router = EventRouter::new();
            let mut screen = Screen::new(width, 16);
            let frame = screen
                .frame(&mut host, &mut router, [], (width, 16))
                .expect("branch choices");
            assert!(
                frame.text().contains("Branch after a turn"),
                "{}",
                frame.text()
            );
            assert!(frame.text().contains("files unchanged"), "{}", frame.text());
            assert!(!frame.text().contains("open turn"));
            router.focus_next(host.cx());
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [
                        sigil_tui::key_down(Key::ArrowDown),
                        sigil_tui::key_down(Key::Enter),
                    ],
                    (width, 16),
                )
                .expect("branch selection");
            assert!(
                matches!(rx.try_recv(), Ok(Cmd::Branch { session, turn: 0 }) if session == current)
            );
            assert!(!app.search.open.peek());
            assert_eq!(app.draft.peek(), "keep this draft");
            assert_eq!(app.caret.peek(), 4);
            assert!(rx.try_recv().is_err());
            app.submit();
            app.queue();
            app.interrupt();
            assert_eq!(app.draft.peek(), "keep this draft");
            assert!(rx.try_recv().is_err());
            app.search.open.set(true);
            app.jump();
            assert!(rx.try_recv().is_err());
            assert!(!app.search.open.peek());
            app.model.borrow_mut().prompt_tokens = 100;
            app.model.borrow_mut().completion_tokens = 50;
            app.model.borrow_mut().rounds = 5;
            app.update(Msg::Reset);
            assert_eq!(app.model.borrow().prompt_tokens, 0);
            assert_eq!(app.model.borrow().completion_tokens, 0);
            assert_eq!(app.model.borrow().rounds, 0);
            app.update(Msg::User("repeated prompt".to_string()));
            app.update(Msg::Session("new branch".to_string()));
            app.update(Msg::Busy(None));
            app.submit();
            assert!(
                matches!(rx.try_recv(), Ok(Cmd::Submit(input)) if input.text == "keep this draft")
            );
        });
    }
}

#[test]
fn session_tree_keeps_relatives_in_order_and_resumes_the_parent() {
    for width in [40, 100] {
        sigil::reactive::Runtime::scope("summoner-session-tree", || {
            let (cmd, mut rx) = unbounded_channel();
            let app = App::new(info(), PathBuf::from("."), cmd);
            app.resize(width, 16);
            let current = app.model.borrow().session.clone();
            app.replace("/tree".to_string());
            app.submit();
            assert!(matches!(rx.try_recv(), Ok(Cmd::ListSessions)));
            app.replace("tree draft".to_string());
            let session = |id: &str, name: &str, parent: Option<&str>| {
                (
                    crate::session::SessionInfo {
                        id: id.to_string(),
                        path: PathBuf::from(id),
                        cwd: None,
                        model: None,
                        preview: String::new(),
                        name: Some(name.to_string()),
                        parent: parent.map(str::to_string),
                        depth: 0,
                    },
                    true,
                )
            };
            app.update(Msg::Sessions(Ok(vec![
                session("unrelated", "Unrelated", None),
                session("sibling", "Other branch", Some("parent")),
                session(&current, "This branch", Some("parent")),
                session("parent", "Original task", None),
            ])));
            assert_eq!(
                app.sessions
                    .rows()
                    .iter()
                    .map(|info| info.id.as_str())
                    .collect::<Vec<_>>(),
                ["parent", "sibling", &current]
            );
            let tree = app.clone();
            let mut host = Host::cells();
            host.mount(move |cx: &mut Cx| cx.add(root(tree)));
            let mut router = EventRouter::new();
            let mut screen = Screen::new(width, 16);
            let frame = screen
                .frame(&mut host, &mut router, [], (width, 16))
                .expect("related sessions");
            assert!(frame.text().contains("Original task"), "{}", frame.text());
            assert!(frame.text().contains("current"), "{}", frame.text());
            assert!(frame.text().contains("from arent"), "{}", frame.text());
            assert!(!frame.text().contains("Unrelated"));
            router.focus_next(host.cx());
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::key_down(Key::Enter)],
                    (width, 16),
                )
                .expect("keep current");
            assert!(!app.sessions.open.peek());
            assert!(rx.try_recv().is_err());
            app.sessions.open.set(true);
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [
                        sigil_tui::key_down(Key::ArrowUp),
                        sigil_tui::key_down(Key::ArrowUp),
                        sigil_tui::key_down(Key::Enter),
                    ],
                    (width, 16),
                )
                .expect("return to parent");
            assert!(
                matches!(rx.try_recv(), Ok(Cmd::Command(command)) if command == "/resume parent")
            );
            assert_eq!(app.draft.peek(), "tree draft");
        });
    }
}

#[test]
fn resume_picker_searches_projects_and_resumes_the_selected_session() {
    for width in [40, 100] {
        sigil::reactive::Runtime::scope("summoner-session-picker", || {
            let (cmd, mut rx) = unbounded_channel();
            let app = App::new(info(), PathBuf::from("."), cmd);
            app.resize(width, 16);
            app.replace("/resume".to_string());
            app.submit();
            assert!(matches!(rx.try_recv(), Ok(Cmd::ListSessions)));
            let session = |id: &str, name: &str, cwd: &str| crate::session::SessionInfo {
                id: id.to_string(),
                name: Some(name.to_string()),
                cwd: Some(cwd.to_string()),
                model: Some("test-model".to_string()),
                preview: "Fix a parser regression".to_string(),
                path: PathBuf::from(id),
                parent: None,
                depth: 0,
            };
            app.update(Msg::Sessions(Ok(vec![
                (session("local-id", "Current task", "/project"), true),
                (session("remote-id", "Parser work", "/other-project"), false),
            ])));
            let tree = app.clone();
            let mut host = Host::cells();
            host.mount(move |cx: &mut Cx| cx.add(root(tree)));
            let mut router = EventRouter::new();
            let mut screen = Screen::new(width, 16);
            let frame = screen
                .frame(&mut host, &mut router, [], (width, 16))
                .expect("picker")
                .text();
            assert!(frame.contains("Current task"), "{frame}");
            assert!(!frame.contains("Parser work"), "{frame}");
            router.focus_next(host.cx());
            let frame = screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::key_down(Key::Tab)],
                    (width, 16),
                )
                .expect("all projects")
                .text();
            assert!(frame.contains("Parser work"), "{frame}");
            if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
                println!("session picker {width}x16:\n{frame}");
            }
            let keys = "other parser".chars().filter_map(|ch| {
                sigil_tui::map_key(&TerminalKeyEvent::new(
                    KeyCode::Char(ch),
                    KeyModifiers::NONE,
                ))
            });
            let frame = screen
                .frame(&mut host, &mut router, keys, (width, 16))
                .expect("search")
                .text();
            assert!(frame.contains("Parser work"), "{frame}");
            assert!(!frame.contains("Current task"), "{frame}");
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::key_down(Key::Enter)],
                    (width, 16),
                )
                .expect("resume");
            assert!(
                matches!(rx.try_recv(), Ok(Cmd::Command(command)) if command == "/resume remote-id")
            );
            assert!(!app.sessions.open.peek());
        });
    }
}

#[test]
fn session_switch_keeps_unsent_text_and_images_until_the_user_discards_them() {
    sigil::reactive::Runtime::scope("summoner-resume-draft", || {
        let dir = tempfile::tempdir().expect("workspace");
        let image = dir.path().join("draft.png");
        image::RgbImage::new(1, 1)
            .save(&image)
            .expect("draft image");
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), dir.path().to_path_buf(), cmd);
        let session = app.model.borrow().session.clone();
        assert!(app.paste("draft.png"));
        app.replace("unfinished 🦀".to_string());
        app.caret.set(3);
        for draft in ["unfinished 🦀", ""] {
            app.update(Msg::Resume("selected session".to_string()));
            assert!(!app.quit.peek());
            assert!(app.resume.borrow().is_none());
            assert_eq!(app.model.borrow().session, session);
            assert_eq!(app.draft.peek(), draft);
            assert!(app.model.borrow().transcript().contains("Draft kept"));
            app.replace(String::new());
        }
        app.interrupt();
        assert!(!app.quit.peek(), "discarding an image draft must not exit");
        assert!(
            app.model
                .borrow()
                .transcript()
                .contains("Attached images were removed")
        );
        app.update(Msg::Resume("selected session".to_string()));
        assert_eq!(app.resume.borrow().as_deref(), Some("selected session"));
        assert!(app.quit.peek());
        assert!(rx.try_recv().is_err());
    });
}

#[test]
fn resume_picker_keeps_drafts_on_cancel_and_explains_empty_or_failed_lists() {
    sigil::reactive::Runtime::scope("summoner-session-picker-recovery", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(80, 16);
        app.replace("keep this draft".to_string());
        app.sessions.start(&app.model.borrow().session, false);
        app.update(Msg::Sessions(Ok(Vec::new())));
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 16);
        let frame = screen
            .frame(&mut host, &mut router, [], (80, 16))
            .expect("empty list")
            .text();
        assert!(frame.contains("Tab shows all projects"), "{frame}");
        router.focus_next(host.cx());
        app.update(Msg::Sessions(Err(
            "Cannot read sessions: permission denied".to_string(),
        )));
        let frame = screen
            .frame(&mut host, &mut router, [], (80, 16))
            .expect("failure")
            .text();
        assert!(frame.contains("permission denied"), "{frame}");
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (80, 16),
            )
            .expect("empty selection");
        assert!(rx.try_recv().is_err());
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Escape)],
                (80, 16),
            )
            .expect("cancel");
        assert_eq!(app.draft.peek(), "keep this draft");
        assert!(!app.quit.peek());
        assert!(!app.sessions.open.peek());
    });
}

#[test]
fn file_completion_attaches_a_quoted_path_and_preserves_surrounding_text() {
    sigil::reactive::Runtime::scope("summoner-file-completion", || {
        let dir = tempfile::tempdir().expect("workspace");
        std::fs::create_dir(dir.path().join("docs")).expect("docs");
        std::fs::write(
            dir.path().join("docs/design notes.md"),
            "actual file contents",
        )
        .expect("file");
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), dir.path().to_path_buf(), cmd);
        app.resize(40, 12);
        app.replace("review @dn please".to_string());
        app.caret.set("review @dn".len());
        app.update(Msg::Files(vec![
            "docs/design notes.md".to_string(),
            "src/main.rs".to_string(),
        ]));
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(40, 12);
        let frame = screen
            .frame(&mut host, &mut router, [], (40, 12))
            .expect("initial frame")
            .text();
        assert!(frame.contains("docs/design notes.md"), "{frame}");
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("file completion 40x12:\n{frame}");
        }
        router.focus_next(host.cx());
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Tab)],
                (40, 12),
            )
            .expect("completion");
        assert_eq!(app.draft.peek(), "review @\"docs/design notes.md\" please");
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (40, 12),
            )
            .expect("submit");
        let input = std::iter::from_fn(|| rx.try_recv().ok())
            .find_map(|cmd| match cmd {
                Cmd::Submit(input) => Some(input),
                _ => None,
            })
            .expect("submitted attachment");
        assert_eq!(
            input.text,
            "review @\"docs/design notes.md\" please\n\n--- docs/design notes.md ---\nactual file contents\n"
        );
        app.update(Msg::Busy(None));
        app.replace("review @\"docs/design notes.md\" please".to_string());
        std::fs::remove_file(dir.path().join("docs/design notes.md")).expect("removed candidate");
        let failed = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (40, 12),
            )
            .expect("missing file")
            .text();
        assert_eq!(app.draft.peek(), "review @\"docs/design notes.md\" please");
        assert!(
            !std::iter::from_fn(|| rx.try_recv().ok()).any(|cmd| matches!(cmd, Cmd::Submit(_)))
        );
        assert!(failed.contains("not a readable file"), "{failed}");
        app.replace("inspect @removed".to_string());
        app.update(Msg::Files(vec!["removed.txt".to_string()]));
        assert!(app.complete());
        assert_eq!(app.draft.peek(), "inspect @\"removed.txt\" ");
        app.submit();
        assert_eq!(app.draft.peek(), "inspect @\"removed.txt\" ");
        assert!(
            !std::iter::from_fn(|| rx.try_recv().ok()).any(|cmd| matches!(cmd, Cmd::Submit(_)))
        );
    });
}

#[test]
fn prompt_recall_preserves_the_draft_and_multiline_navigation() {
    sigil::reactive::Runtime::scope("summoner-prompt-recall", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.update(Msg::User("first prompt".to_string()));
        app.update(Msg::User("second prompt".to_string()));
        app.replace("unfinished\nsecond line".to_string());
        app.resize(80, 24);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("initial frame");
        router.focus_next(host.cx());
        for (key, expected) in [
            (Key::ArrowUp, "unfinished\nsecond line"),
            (Key::ArrowUp, "second prompt"),
            (Key::ArrowUp, "first prompt"),
            (Key::ArrowDown, "second prompt"),
            (Key::ArrowDown, "unfinished\nsecond line"),
        ] {
            screen
                .frame(&mut host, &mut router, [sigil_tui::key_down(key)], (80, 24))
                .expect("recall frame");
            assert_eq!(app.draft.peek(), expected);
        }
        assert_eq!(app.caret.peek(), "unfinished".len());
        assert!(rx.try_recv().is_err(), "recalling must not send a prompt");
    });
}

#[test]
fn slash_menu_routes_arrows_tab_and_escape() {
    sigil::reactive::Runtime::scope("mage-command-menu-keys-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(80, 24);
        app.replace("/".to_string());
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

        let selected = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::ArrowDown)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(selected.text().contains("› /settings"));

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Tab)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.draft.peek(), "/settings");

        let hidden = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Escape)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(!hidden.text().contains("commands"));
        assert!(!app.settings_open.peek());
    });
}

#[test]
fn escape_cancels_busy_turn_with_slash_draft() {
    sigil::reactive::Runtime::scope("mage-busy-slash-escape-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(80, 24);
        app.replace("/".to_string());
        app.model.borrow_mut().busy_since = Some((Activity::Turn, Instant::now()));
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
                [sigil_tui::key_down(Key::Escape)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(matches!(rx.try_recv(), Ok(Cmd::Cancel)));
    });
}

#[test]
fn newline_replaces_selection_and_undo_restores_it_in_both_editors() {
    for editing in [false, true] {
        sigil::reactive::Runtime::scope("summoner-newline-undo", || {
            let (cmd, _rx) = unbounded_channel();
            let app = App::new(info(), PathBuf::from("."), cmd);
            app.resize(80, 24);
            if editing {
                app.update(Msg::View {
                    path: "notes.md".to_string(),
                    file: PathBuf::from("notes.md"),
                    rope: sigil_ide_core::buffer::Rope::from_str("replace me"),
                    added: Vec::new(),
                    image: None,
                });
                app.edit_start();
            } else {
                app.replace("replace me".to_string());
            }
            let tree = app.clone();
            let mut host = Host::cells();
            host.mount(move |cx: &mut Cx| cx.add(root(tree)));
            let mut router = EventRouter::new();
            let mut screen = Screen::new(80, 24);
            screen
                .frame(&mut host, &mut router, [], (80, 24))
                .expect("initial frame");
            router.focus_next(host.cx());
            let key = |code, modifiers| {
                sigil_tui::map_key(&TerminalKeyEvent::new(code, modifiers)).expect("key")
            };
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [key(KeyCode::Char('a'), KeyModifiers::CONTROL)],
                    (80, 24),
                )
                .expect("select all");
            let newline = if editing {
                key(KeyCode::Enter, KeyModifiers::NONE)
            } else {
                key(KeyCode::Char('j'), KeyModifiers::CONTROL)
            };
            screen
                .frame(&mut host, &mut router, [newline], (80, 24))
                .expect("newline");
            let value = if editing { app.view_text } else { app.draft };
            assert_eq!(value.peek(), "\n", "selection must be replaced");
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [key(KeyCode::Char('z'), KeyModifiers::CONTROL)],
                    (80, 24),
                )
                .expect("undo");
            assert_eq!(
                value.peek(),
                "replace me",
                "undo restores the selected text"
            );
        });
    }
}

#[test]
fn composer_maps_enter_and_newline() {
    sigil::reactive::Runtime::scope("mage-composer-key-parity-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
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

        app.replace("line one".to_string());
        let newline = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::CONTROL,
        ))
        .unwrap_or_else(|| panic!("ctrl+j did not map"));
        screen
            .frame(&mut host, &mut router, [newline], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.draft.peek(), "line one\n");

        app.replace("send now".to_string());
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(matches!(rx.try_recv(), Ok(Cmd::Submit(_))));
        assert!(app.draft.peek().is_empty());
    });
}

#[test]
fn newline_closes_suggestions_before_enter() {
    sigil::reactive::Runtime::scope("mage-slash-newline-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(80, 24);
        app.replace("/help".to_string());
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
        let newline = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::CONTROL,
        ))
        .unwrap_or_else(|| panic!("ctrl+j did not map"));

        screen
            .frame(&mut host, &mut router, [newline], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(app.draft.peek().is_empty());
    });
}

#[test]
fn composer_hotkeys_keep_completion_queue_settings_and_editor_distinct() {
    sigil::reactive::Runtime::scope("mage-composer-hotkey-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
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

        app.model.borrow_mut().busy_since = Some((Activity::Turn, Instant::now()));
        app.replace("queue next".to_string());
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Tab)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(matches!(rx.try_recv(), Ok(Cmd::Queue(input)) if input.text == "queue next"));

        app.model.borrow_mut().busy_since = None;
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::map_key(&TerminalKeyEvent::new(
                    KeyCode::BackTab,
                    KeyModifiers::SHIFT,
                ))
                .unwrap_or_else(|| panic!("shift+tab did not map"))],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetMode(true))));

        let ctrl = |ch| {
            sigil_tui::map_key(&TerminalKeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::CONTROL,
            ))
            .unwrap_or_else(|| panic!("ctrl+{ch} did not map"))
        };
        screen
            .frame(&mut host, &mut router, [ctrl('o')], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(app.settings_open.peek());
        screen
            .frame(&mut host, &mut router, [ctrl('o')], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(!app.settings_open.peek());

        let selected = screen
            .frame(&mut host, &mut router, [ctrl('t')], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(app.select.peek());
        assert!(selected.text().contains("select on · mouse to terminal"));
        screen
            .frame(&mut host, &mut router, [ctrl('t')], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(!app.select.peek());

        screen
            .frame(&mut host, &mut router, [ctrl('g')], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(app.editor_requested.peek());
    });
}

#[test]
fn visible_caret_tracks_editor_navigation() {
    sigil::reactive::Runtime::scope("mage-visible-caret-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(40, 12);
        app.replace("hello".to_string());
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(40, 12);
        screen
            .frame(&mut host, &mut router, [], (40, 12))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        router.focus_next(host.cx());
        let frame = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::ArrowLeft)],
                (40, 12),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();

        assert_eq!(app.caret.peek(), 4);
        assert!(
            frame.text().contains("hell▌o"),
            "caret did not move:\n{}",
            frame.text()
        );

        let newline = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::CONTROL,
        ))
        .unwrap_or_else(|| panic!("ctrl+j did not map"));
        screen
            .frame(&mut host, &mut router, [newline], (40, 12))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.draft.peek(), "hell\no");
        assert_eq!(app.caret.peek(), 5);
    });
}
