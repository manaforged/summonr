use super::*;

#[test]
fn peer_updates_do_not_invalidate_the_history_cache() {
    sigil::reactive::Runtime::scope("summonr-peers-skip-rerender", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(&app, Msg::System("before".to_string()));
        let stamp = app.model.borrow().entries.stamp;
        apply(&app, Msg::Peers(2));
        assert_eq!(app.model.borrow().entries.stamp, stamp);
        apply(&app, Msg::System("after".to_string()));
        assert_ne!(app.model.borrow().entries.stamp, stamp);
    });
}
#[test]
fn an_empty_patch_has_a_visible_completed_state() {
    sigil::reactive::Runtime::scope("summonr-empty-patch", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(120, 30);
        app.toggle_pane(super::super::pane::Pane::Diff);
        app.diff_open("unchanged.txt".to_string());
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 30);
        let loading = screen
            .frame(&mut host, &mut router, [], (120, 30))
            .expect("loading")
            .text();
        assert!(loading.contains("loading…"), "{loading}");
        assert!(!loading.contains("no changes"), "{loading}");
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "unchanged.txt".to_string(),
                result: Ok(String::new()),
            },
        );
        for width in [120, 220] {
            app.resize(width, 30);
            let empty = screen
                .frame(&mut host, &mut router, [], (width, 30))
                .expect("empty patch")
                .text();
            assert!(empty.contains("no changes"), "{empty}");
            assert!(!empty.contains("loading…"), "{empty}");
            assert!(empty.contains("unchanged.txt"), "{empty}");
        }
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "unchanged.txt".to_string(),
                result: Ok("+new content\n".to_string()),
            },
        );
        let changed = screen
            .frame(&mut host, &mut router, [], (220, 30))
            .expect("changed patch")
            .text();
        assert!(changed.contains("new content"), "{changed}");
        assert!(!changed.contains("no changes"), "{changed}");
    });
}

#[test]
fn copying_a_loading_patch_requests_the_selected_file() {
    sigil::reactive::Runtime::scope("summonr-patch-copy-loading", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.diff_open("selected.txt".to_string());
        assert!(matches!(rx.try_recv(), Ok(Cmd::Patch(None, path)) if path == "selected.txt"));
        app.copy_patch();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Patch(None, path)) if path == "selected.txt"));
        assert!(app.copy.peek().is_none());
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "other.txt".to_string(),
                result: Ok("+other\n".to_string()),
            },
        );
        assert!(app.copy.peek().is_none());
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "selected.txt".to_string(),
                result: Ok("+selected\n".to_string()),
            },
        );
        assert_eq!(app.copy.peek().as_deref(), Some("+selected\n"));
        app.copy.set(None);
        app.diff_open("first.txt".to_string());
        app.copy_patch();
        app.diff_open("next.txt".to_string());
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "first.txt".to_string(),
                result: Ok("+requested\n".to_string()),
            },
        );
        assert_eq!(app.copy.peek().as_deref(), Some("+requested\n"));
        assert!(
            app.model.borrow().patch.is_none(),
            "copying the requested file must not replace the newly opened pane"
        );
    });
}

#[test]
fn patch_errors_wrap_scroll_and_leave_the_clipboard_untouched() {
    sigil::reactive::Runtime::scope("summonr-patch-errors", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(120, 30);
        app.toggle_pane(super::super::pane::Pane::Diff);
        app.diff_open("00-nested/".to_string());
        let error = "open 00-nested/ as a separate workspace to inspect its changes";
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "00-nested/".to_string(),
                result: Err(error.to_string()),
            },
        );
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 30);
        let frame = screen
            .frame(&mut host, &mut router, [], (120, 30))
            .expect("patch error")
            .text();
        assert!(frame.contains("patch unavailable"), "{frame}");
        let pane = frame
            .lines()
            .filter_map(|line| line.rsplit_once('│').map(|(_, pane)| pane.trim()))
            .collect::<Vec<_>>()
            .join(" ");
        let pane = pane.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(pane.contains(error), "{pane}");
        assert!(!frame.contains("loading…"), "{frame}");
        app.copy_patch();
        assert!(app.copy.peek().is_none());
        assert!(
            app.model
                .borrow()
                .transcript()
                .contains(&format!("cannot copy patch: {error}"))
        );
        let long = format!("{} end of patch failure", "unavailable file ".repeat(80));
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "00-nested/".to_string(),
                result: Err(long),
            },
        );
        app.pane_move(1000);
        let frame = screen
            .frame(&mut host, &mut router, [], (120, 30))
            .expect("scrolled error")
            .text();
        assert!(frame.contains("patch failure"), "{frame}");
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "00-nested/".to_string(),
                result: Ok("+short replacement\n".to_string()),
            },
        );
        let frame = screen
            .frame(&mut host, &mut router, [], (120, 30))
            .expect("shorter patch")
            .text();
        assert!(frame.contains("+short replacement"), "{frame}");
        app.diff_open("new.txt".to_string());
        app.copy_patch();
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "new.txt".to_string(),
                result: Err("unreadable".to_string()),
            },
        );
        assert!(app.copy.peek().is_none());
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "new.txt".to_string(),
                result: Ok("+recovered\n".to_string()),
            },
        );
        assert!(
            app.copy.peek().is_none(),
            "a failed copy cannot complete on a later refresh"
        );
        app.copy_patch();
        assert_eq!(app.copy.peek().as_deref(), Some("+recovered\n"));
        app.copy.set(None);
        app.pane_back();
        app.copy_patch();
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: String::new(),
                result: Ok(String::new()),
            },
        );
        assert!(app.copy.peek().is_none());
        assert!(
            app.model
                .borrow()
                .transcript()
                .contains("there are no patch changes to copy")
        );
    });
}

#[test]
fn patch_responses_follow_the_requested_workspace() {
    sigil::reactive::Runtime::scope("summonr-patch-root", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        let first = PathBuf::from("first");
        let second = PathBuf::from("second");
        app.tree_show(first.clone(), "first".to_string());
        app.diff_open("file.txt".to_string());
        app.copy_patch();
        app.tree_show(second.clone(), "second".to_string());
        app.diff_open("file.txt".to_string());
        app.copy_patch();
        apply(
            &app,
            Msg::Patch {
                root: Some(first.clone()),
                path: "file.txt".to_string(),
                result: Ok("+first\n".to_string()),
            },
        );
        assert!(app.copy.peek().is_none());
        assert!(app.model.borrow().patch.is_none());
        apply(
            &app,
            Msg::Patch {
                root: Some(second),
                path: "file.txt".to_string(),
                result: Ok("+second\n".to_string()),
            },
        );
        assert_eq!(app.copy.peek().as_deref(), Some("+second\n"));
        app.copy.set(None);
        apply(
            &app,
            Msg::Patch {
                root: Some(first),
                path: "file.txt".to_string(),
                result: Ok("+stale\n".to_string()),
            },
        );
        app.copy_patch();
        assert_eq!(app.copy.peek().as_deref(), Some("+second\n"));
        app.copy.set(None);
        app.toggle_pane(super::super::pane::Pane::Diff);
        app.toggle_pane(super::super::pane::Pane::Diff);
        app.copy_patch();
        assert!(
            app.copy.peek().is_none(),
            "switching to the primary workspace must invalidate the cached worktree patch"
        );
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "file.txt".to_string(),
                result: Ok("+primary\n".to_string()),
            },
        );
        assert_eq!(app.copy.peek().as_deref(), Some("+primary\n"));
    });
}

#[test]
fn wide_patch_scrolling_stops_at_the_last_visible_pair() {
    sigil::reactive::Runtime::scope("summonr-patch-pair-scroll", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(220, 30);
        app.toggle_pane(super::super::pane::Pane::Diff);
        app.diff_open("file.txt".to_string());
        apply(
            &app,
            Msg::Patch {
                root: None,
                path: "file.txt".to_string(),
                result: Ok(
                    "@@ -1,2 +1,2 @@\n-old first\n-old last\n+new first\n+new last\n".to_string(),
                ),
            },
        );
        app.pane_edge(true);
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(220, 30);
        let frame = screen
            .frame(&mut host, &mut router, [], (220, 30))
            .expect("last pair")
            .text();
        assert!(
            frame.contains("old last") && frame.contains("new last"),
            "{frame}"
        );
        app.pane_move(-1);
        let frame = screen
            .frame(&mut host, &mut router, [], (220, 30))
            .expect("previous pair")
            .text();
        assert!(
            frame.contains("old first") && frame.contains("new first"),
            "{frame}"
        );
        app.resize(120, 30);
        app.pane_edge(true);
        let frame = screen
            .frame(&mut host, &mut router, [], (120, 30))
            .expect("unified end")
            .text();
        assert!(frame.contains("+new last"), "{frame}");
        app.resize(220, 30);
        let frame = screen
            .frame(&mut host, &mut router, [], (220, 30))
            .expect("split after resize")
            .text();
        assert!(
            frame.contains("old last") && frame.contains("new last"),
            "{frame}"
        );
    });
}

#[test]
fn restored_panes_request_their_initial_data() {
    sigil::reactive::Runtime::scope("summonr-restored-pane-data", || {
        for pane in ["diff", "files", "worktrees", "ledger"] {
            let (cmd, mut rx) = unbounded_channel();
            let mut start = info();
            start.prefs.layout = format!("chat | {pane} 40%");
            let app = App::new(start, PathBuf::from("."), cmd);
            app.load();
            match pane {
                "diff" => assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None)))),
                "files" => assert!(matches!(rx.try_recv(), Ok(Cmd::ListFiles))),
                "worktrees" => assert!(matches!(rx.try_recv(), Ok(Cmd::Trees))),
                _ => assert!(rx.try_recv().is_err()),
            }
        }
    });
}

#[test]
fn diff_failure_settles_loading_and_retry_restores_the_pane() {
    sigil::reactive::Runtime::scope("summonr-diff-recovery", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(120, 40);
        app.toggle_pane(super::super::pane::Pane::Diff);
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        let frame = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .expect("loading frame")
            .text();
        assert!(frame.contains("loading…"), "{frame}");
        apply(
            &app,
            Msg::DiffFailed("git failed: permission denied".to_string()),
        );
        let frame = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .expect("error frame")
            .text();
        assert!(frame.contains("diff unavailable"), "{frame}");
        assert!(frame.contains("git failed: permission denied"), "{frame}");
        assert!(frame.contains("reopen Diff to retry"), "{frame}");
        assert!(!frame.contains("loading…"), "{frame}");
        assert!(!frame.contains("no changes"), "{frame}");
        app.toggle_pane(super::super::pane::Pane::Diff);
        app.toggle_pane(super::super::pane::Pane::Diff);
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
        apply(
            &app,
            Msg::Diff(crate::diff::Diff {
                base: "empty tree".to_string(),
                files: vec![crate::diff::Stat {
                    path: "new.txt".to_string(),
                    added: 1,
                    new: true,
                    ..crate::diff::Stat::default()
                }],
            }),
        );
        let frame = screen
            .frame(&mut host, &mut router, [], (120, 40))
            .expect("recovered frame")
            .text();
        assert!(frame.contains("new.txt (new)"), "{frame}");
        assert!(!frame.contains("diff unavailable"), "{frame}");
    });
}

#[test]
fn restored_conversation_keeps_roles_and_one_copy_of_the_reply() {
    sigil::reactive::Runtime::scope("summonr-restored-view", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        apply(&app, Msg::User("fix arithmetic".to_string()));
        apply(
            &app,
            Msg::Event(Event::Assistant {
                text: "Fixed it.".to_string(),
            }),
        );
        apply(
            &app,
            Msg::Receipt("Fixed it.\n\nTurn receipt\n- Changed: calculator.py".to_string()),
        );
        let text = app.model.borrow().transcript();
        assert!(text.contains("fix arithmetic"));
        assert!(text.contains("Changed: calculator.py"));
        assert_eq!(text.matches("Fixed it.").count(), 1);
        app.replace("/copy".to_string());
        app.submit();
        assert_eq!(app.copy.peek().as_deref(), Some("Fixed it."));
        assert!(
            matches!(app.model.borrow().entries.first(), Some(Entry::User(text)) if text == "fix arithmetic")
        );
        apply(&app, Msg::Cleared);
        apply(&app, Msg::User("next task".to_string()));
        let text = app.model.borrow().transcript();
        assert!(text.contains("next task"));
        assert!(!text.contains("fix arithmetic"));
    });
}

#[test]
fn configured_endpoint_list_drives_switching() {
    sigil::reactive::Runtime::scope("mage-endpoint-cycle-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.cycle_endpoint();
        let Ok(next) = rx.try_recv() else {
            panic!("endpoint command missing");
        };
        let Cmd::SetEndpoint(name) = next else {
            panic!("expected SetEndpoint");
        };
        assert!(
            crate::endpoint::resolve(Some(&name), None).is_ok(),
            "cycle sent unresolved seat `{name}`"
        );
        assert_ne!(name, "glm", "cycle stayed on the current seat");
    });
}

#[test]
fn streamed_events_become_a_markdown_transcript() {
    sigil::reactive::Runtime::scope("mage-event-view-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(&app, Msg::Thinking("checking".to_string()));
        apply(&app, Msg::Delta("Hello".to_string()));
        apply(&app, Msg::Delta(" world".to_string()));
        apply(
            &app,
            Msg::Event(Event::Assistant {
                text: "Hello world".to_string(),
            }),
        );
        let text = app.model.borrow().transcript();
        assert!(text.contains("**Thinking**"));
        assert!(text.contains("Hello world"));
    });
}

#[test]
fn turn_receipt_does_not_duplicate_the_streamed_reply() {
    sigil::reactive::Runtime::scope("mage-stream-receipt-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(&app, Msg::Delta("Hello world".to_string()));
        apply(
            &app,
            Msg::Event(Event::Receipt {
                receipt: crate::Receipt::new(
                    Vec::new(),
                    Vec::new(),
                    "non-Git workspace".to_string(),
                    &[],
                ),
            }),
        );
        apply(
            &app,
            Msg::Event(Event::Assistant {
                text: "Hello world".to_string(),
            }),
        );

        assert_eq!(
            app.model
                .borrow()
                .transcript()
                .matches("Hello world")
                .count(),
            1
        );
        let model = app.model.borrow();
        let replies: Vec<_> = model
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Assistant { text, done } => Some((text.as_str(), *done)),
                _ => None,
            })
            .collect();
        assert_eq!(replies, vec![("Hello world", true)]);
    });
}

#[test]
fn copy_hotkey_uses_the_last_reply() {
    sigil::reactive::Runtime::scope("mage-copy-reply-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(
            &app,
            Msg::Event(Event::Assistant {
                text: "first reply".to_string(),
            }),
        );
        apply(
            &app,
            Msg::Event(Event::Assistant {
                text: "latest reply".to_string(),
            }),
        );

        assert!(!app.route_hotkey('y'));
        assert!(app.route_hotkey('b'));
        assert_eq!(app.copy.peek().as_deref(), Some("latest reply"));
    });
}

#[test]
fn copy_all_uses_the_full_transcript() {
    sigil::reactive::Runtime::scope("mage-copy-transcript-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(
            &app,
            Msg::Event(Event::Assistant {
                text: "first reply".to_string(),
            }),
        );
        apply(
            &app,
            Msg::Event(Event::Assistant {
                text: "latest reply".to_string(),
            }),
        );

        app.replace("/copy all".to_string());
        app.submit();
        let copied = app.copy.peek().unwrap_or_default();
        assert!(copied.contains("first reply"), "{copied}");
        assert!(copied.contains("latest reply"), "{copied}");
    });
}

#[test]
fn copy_reports_when_there_is_no_reply() {
    sigil::reactive::Runtime::scope("mage-copy-empty-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);

        app.replace("/copy".to_string());
        app.submit();

        assert!(
            app.model
                .borrow()
                .transcript()
                .contains("there is no reply to copy")
        );
        assert!(app.copy.peek().is_none());
    });
}

#[test]
fn copy_rejects_an_unknown_scope() {
    sigil::reactive::Runtime::scope("mage-copy-scope-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);

        app.replace("/copy turn".to_string());
        app.submit();

        assert!(
            app.model
                .borrow()
                .transcript()
                .contains("usage: /copy [all]")
        );
        assert!(app.copy.peek().is_none());
    });
}

#[test]
fn tool_result_updates_the_existing_call() {
    sigil::reactive::Runtime::scope("mage-tool-view-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(
            &app,
            Msg::Event(Event::ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: r#"{"path":"README.md"}"#.to_string(),
            }),
        );
        let output = format!(
            "early error\n{}\nlast",
            (1..30)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        apply(
            &app,
            Msg::Event(Event::ToolResult {
                id: "call-1".to_string(),
                success: true,
                mutated: false,
                output: output.clone(),
            }),
        );
        let text = app.model.borrow().transcript();
        assert!(text.contains("✓ **read_file** `README.md`"));
        assert!(text.contains(&output), "transcript retains the full result");
        let render = || {
            let tree = app.clone();
            sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 50, &[])
                .expect("tool output frame")
        };
        app.resize(100, 50);
        let compact = render();
        assert!(compact.contains("last"));
        assert!(!compact.contains("early error"));
        app.replace("unrelated draft".to_string());
        app.search.start("early error");
        assert_eq!(
            app.search
                .rows(&app.model.borrow())
                .expect("search tool output")
                .len(),
            1
        );
        app.jump();
        let expanded = render();
        for line in ["early error", "line 15", "last"] {
            assert!(expanded.contains(line), "missing {line}:\n{expanded}");
        }
        assert_eq!(app.draft.peek(), "unrelated draft");
        app.toggle_tools();
        assert!(!render().contains("early error"));
        app.replace("/copy all".to_string());
        app.submit();
        assert!(app.copy.peek().expect("copy transcript").contains(&output));
    });
}

#[test]
fn hidden_reasoning_never_enters_the_transcript() {
    sigil::reactive::Runtime::scope("mage-reasoning-pref-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut info = info();
        info.prefs.reasoning = crate::prefs::Reasoning::Off;
        let app = App::new(info, PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(&app, Msg::Thinking("private chain".to_string()));
        assert!(!app.model.borrow().transcript().contains("private chain"));
    });
}

#[test]
fn busy_submit_steers_and_queue_stays_distinct() {
    sigil::reactive::Runtime::scope("mage-submit-routing-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(&app, Msg::Busy(Some(Activity::Turn)));
        app.replace("steer now".to_string());
        app.submit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Steer(input)) if input.text == "steer now"));
        app.replace("then do this".to_string());
        app.queue();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Queue(input)) if input.text == "then do this"));
    });
}

#[test]
fn failed_turn_releases_busy_before_the_next_submit() {
    sigil::reactive::Runtime::scope("mage-failed-turn-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(&app, Msg::Busy(Some(Activity::Turn)));
        apply(&app, Msg::Failed("provider unavailable".to_string()));
        assert!(app.model.borrow().busy_since.is_none());

        app.replace("try again".to_string());
        app.submit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::Submit(input)) if input.text == "try again"));
    });
}

#[test]
fn local_command_keeps_the_splash_visible() {
    sigil::reactive::Runtime::scope("mage-splash-command-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut start = info();
        start.splash = true;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.replace("/settings".to_string());
        app.submit();

        assert!(app.splash.peek());
        assert!(app.settings_open.peek());
        assert!(app.model.borrow().entries.is_empty());
    });
}

#[test]
fn slash_commands_never_become_model_input() {
    sigil::reactive::Runtime::scope("mage-slash-routing-test", || {
        let commands = crate::command_specs()
            .iter()
            .map(|spec| spec.name)
            .chain(std::iter::once("/not-a-command"));
        for line in commands {
            for busy in [false, true] {
                let (cmd, mut rx) = unbounded_channel();
                let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
                app.model.borrow_mut().busy_since = busy.then(|| (Activity::Turn, Instant::now()));
                app.replace(line.to_string());
                app.submit();
                if let Ok(command) = rx.try_recv() {
                    assert!(
                        !matches!(command, Cmd::Submit(_) | Cmd::Steer(_)),
                        "`{line}` became model input while busy={busy}"
                    );
                }
                assert!(
                    !app.model
                        .borrow()
                        .entries
                        .iter()
                        .any(|entry| matches!(entry, Entry::User(text) if text == line)),
                    "`{line}` entered the transcript while busy={busy}"
                );
            }
        }
    });
}

#[test]
fn slash_prefix_completes_from_the_shared_catalog() {
    sigil::reactive::Runtime::scope("mage-command-completion-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.replace("/pl".to_string());

        assert!(app.complete());
        assert_eq!(app.draft.peek(), "/plan");
    });
}

#[test]
fn slash_completion_accepts_and_cycles_the_visible_matches() {
    sigil::reactive::Runtime::scope("mage-command-completion-cycle-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.replace("/".to_string());
        assert!(app.accept());
        assert_eq!(app.draft.peek(), "/help");

        app.replace("/se".to_string());
        assert!(app.complete());
        assert_eq!(app.draft.peek(), "/search");
        assert!(app.complete());
        assert_eq!(app.draft.peek(), "/settings");
        assert!(app.complete());
        assert_eq!(app.draft.peek(), "/session");
        assert!(app.complete());
        assert_eq!(app.draft.peek(), "/search");
    });
}

#[test]
fn slash_model_opens_the_picker_without_starting_a_turn() {
    sigil::reactive::Runtime::scope("mage-model-command-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.splash = true;
        let app = App::new(start, PathBuf::from("/tmp/mage-view-test"), cmd);
        app.splash_revealed.set(true);
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
        app.replace("/model".to_string());

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (100, 32),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(app.editing_model.peek());
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        assert!(app.model.borrow().entries.is_empty());
    });
}

#[test]
fn slash_popup_filters_to_the_typed_prefix() {
    sigil::reactive::Runtime::scope("mage-command-prefix-frame-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.replace("/m".to_string());
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(
            frame.contains("│› /model"),
            "typing /m must suggest /model:\n{frame}"
        );
        assert!(
            frame.contains("tab accept"),
            "prefix menu missing:\n{frame}"
        );
        assert!(
            !frame.contains("│› /help") && !frame.contains("│  /help"),
            "typing /m must not keep the full catalog:\n{frame}"
        );
    });
}

#[test]
fn slash_popup_shows_the_available_command_catalog() {
    sigil::reactive::Runtime::scope("mage-command-catalog-frame-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.replace("/".to_string());
        app.refresh();
        let tree = app.clone();
        let first = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("chat commands 100x32:\n{first}");
        }

        for name in [
            "/help",
            "/model",
            "/diff",
            "/view",
            "/search",
            "/history",
            "/resume",
            "/settings",
            "/login",
        ] {
            assert!(first.contains(name), "{name} missing:\n{first}");
        }
        assert!(first.contains("1–9/21"), "{first}");
        assert!(
            first.contains("type to search"),
            "search hint missing:\n{first}"
        );

        app.shift(-1);
        let tree = app.clone();
        let last = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(last.contains("› /exit"), "catalog did not scroll:\n{last}");
    });
}

#[test]
fn live_model_changes_update_splash_facts() {
    sigil::reactive::Runtime::scope("mage-live-facts-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(
            &app,
            Msg::Model {
                model: "deepseek-r2".to_string(),
                relay: Some("relay.local".to_string()),
            },
        );

        assert!(app.facts.peek().contains("deepseek-r2 · relay.local"));
        assert!(!app.facts.peek().contains("glm-5.3"));
    });
}

#[test]
fn preference_write_failure_is_visible_in_the_transcript() {
    sigil::reactive::Runtime::scope("mage-pref-write-failure-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        let blocker = std::env::temp_dir().join(format!(
            "mage-pref-parent-file-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        if let Err(error) = std::fs::write(&blocker, "not a directory") {
            panic!("failed to create preference blocker: {error}");
        }
        app.prefs_path = Some(blocker.join("config.toml"));
        app.reasoning_command(None);

        assert!(
            app.model
                .borrow()
                .transcript()
                .contains("settings not saved")
        );
        if let Err(error) = std::fs::remove_file(&blocker) {
            panic!("failed to remove preference blocker: {error}");
        }
    });
}

#[test]
fn bye_requests_a_clean_host_exit() {
    sigil::reactive::Runtime::scope("mage-bye-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        assert!(!app.quit.peek());
        apply(&app, Msg::Bye);
        assert!(app.quit.peek());
    });
}

#[test]
fn the_workflow_card_routes_seat_picks_saves_and_cycles() {
    use crate::workflow::Role;
    sigil::reactive::Runtime::scope("mage-workflow-card-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.replace("/workflow".to_string());
        app.submit();
        assert!(app.workflow_open.peek());
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListWorkflows)));
        apply(
            &app,
            Msg::Workflow {
                name: Some("b".to_string()),
                names: vec!["a".to_string(), "b".to_string()],
                seats: vec![None, Some("grok\tgrok-4.6".to_string())],
            },
        );
        assert_eq!(app.workflow_name.peek(), "b");
        assert_eq!(app.workflow_seats.peek().len(), Role::ALL.len());

        app.move_workflow(3);
        assert_eq!(app.workflow_sel.peek(), 3, "review");
        app.pick_workflow_seat();
        assert!(app.editing_model.peek());
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        app.replace_model_draft("glm\tglm-5.3".to_string());
        app.commit_model_edit();
        assert!(!app.editing_model.peek());
        assert!(matches!(
            rx.try_recv(),
            Ok(Cmd::SeatRole(Role::Review, endpoint, model)) if endpoint == "glm" && model == "glm-5.3"
        ));
        assert_eq!(app.model_for.peek(), None);

        app.clear_workflow_seat();
        assert!(matches!(rx.try_recv(), Ok(Cmd::ClearRole(Role::Review))));

        app.cycle_workflow(-1);
        assert!(matches!(rx.try_recv(), Ok(Cmd::Command(line)) if line == "/workflow a"));
        app.cycle_workflow(1);
        assert!(matches!(rx.try_recv(), Ok(Cmd::Command(line)) if line == "/workflow a"));

        app.begin_workflow_save();
        assert!(app.workflow_naming.peek());
        assert_eq!(app.workflow_draft.peek(), "b");
        app.move_workflow(-1);
        assert_eq!(app.workflow_sel.peek(), 3, "naming ignores row moves");
        app.replace_workflow_draft("fast".to_string());
        app.pick_workflow_seat();
        assert!(!app.workflow_naming.peek());
        assert!(matches!(rx.try_recv(), Ok(Cmd::SaveWorkflow(name)) if name == "fast"));

        app.close_workflow();
        assert!(!app.workflow_open.peek());
        app.replace("/workflow fast".to_string());
        app.submit();
        assert!(
            !app.workflow_open.peek(),
            "a named workflow goes to the worker"
        );
        assert!(matches!(rx.try_recv(), Ok(Cmd::Command(line)) if line == "/workflow fast"));
    });
}

#[test]
fn ctrl_c_needs_two_presses_to_quit_and_escape_twice_opens_the_tree() {
    sigil::reactive::Runtime::scope("summonr-repeat-keys", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.splash.set(false);
        app.interrupt();
        assert!(!app.quit.peek(), "one press only arms the quit");
        assert!(
            app.model
                .borrow()
                .transcript()
                .contains("press ctrl+c again to quit"),
            "{}",
            app.model.borrow().transcript()
        );
        app.interrupt();
        assert!(app.quit.peek(), "the second press quits");

        assert!(!app.escape_idle(), "one escape only arms the tree");
        assert!(app.escape_idle(), "the second escape opens the tree");
        assert!(app.sessions.open.peek());
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListSessions)));

        let before = app.model.borrow().prefs.tools;
        app.replace("/tools".to_string());
        app.submit();
        assert_ne!(
            app.model.borrow().prefs.tools,
            before,
            "/tools toggles the output style"
        );
        let reasoning = app.model.borrow().prefs.reasoning;
        app.replace("/reasoning".to_string());
        app.submit();
        assert_ne!(app.model.borrow().prefs.reasoning, reasoning);
    });
}

#[test]
fn a_long_paste_folds_into_a_token_and_expands_on_submit() {
    sigil::reactive::Runtime::scope("summonr-paste-fold", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.splash.set(false);
        assert!(!app.paste_text("two\nlines"), "short pastes stay inline");
        let log = (1..=12)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.replace("look at ".to_string());
        app.caret.set(8);
        assert!(app.paste_text(&log));
        assert_eq!(app.draft.peek(), "look at [pasted 12 lines #1]");
        app.submit();
        let Ok(Cmd::Submit(input)) = rx.try_recv() else {
            panic!("expected a submit");
        };
        assert_eq!(input.text, format!("look at {log}"));
        assert_eq!(
            app.expand_pastes("[pasted 12 lines #1]"),
            "[pasted 12 lines #1]",
            "the store empties once sent"
        );
    });
}

#[test]
fn ctrl_d_in_the_picker_deletes_the_selected_session_on_the_second_press() {
    sigil::reactive::Runtime::scope("summonr-picker-delete", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.splash.set(false);
        app.replace("/resume".to_string());
        app.submit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListSessions)));
        let row = |id: &str| crate::session::SessionInfo {
            id: id.to_string(),
            path: PathBuf::from(format!("/tmp/{id}/session.jsonl")),
            cwd: Some(".".to_string()),
            model: None,
            preview: format!("prompt in {id}"),
            name: None,
            parent: None,
            depth: 0,
        };
        app.update(Msg::Sessions(Ok(vec![
            (row("aaaa1111"), true),
            (row("bbbb2222"), true),
        ])));
        app.sessions.selected.set(1);
        app.delete_session();
        assert!(
            rx.try_recv().is_err(),
            "the first press only arms the delete"
        );
        app.delete_session();
        assert!(matches!(rx.try_recv(), Ok(Cmd::DeleteSession(id)) if id == "bbbb2222"));
    });
}
