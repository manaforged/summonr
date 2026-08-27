use super::*;

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
        let text = app.transcript.peek();
        assert!(text.contains("**Thinking**"));
        assert!(text.contains("Hello world"));
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
        apply(
            &app,
            Msg::Event(Event::ToolResult {
                id: "call-1".to_string(),
                success: true,
                mutated: false,
                output: "first\nlast".to_string(),
            }),
        );
        let text = app.transcript.peek();
        assert!(text.contains("✓ **read_file** `README.md`"));
        assert!(text.contains("last"));
        assert!(!text.contains("first"), "compact output keeps one line");
    });
}

#[test]
fn hidden_reasoning_never_enters_the_transcript() {
    sigil::reactive::Runtime::scope("mage-reasoning-pref-test", || {
        let (cmd, _rx) = unbounded_channel();
        let mut info = info();
        info.prefs.reasoning = false;
        let app = App::new(info, PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(&app, Msg::Thinking("private chain".to_string()));
        assert!(!app.transcript.peek().contains("private chain"));
    });
}

#[test]
fn busy_submit_steers_and_queue_stays_distinct() {
    sigil::reactive::Runtime::scope("mage-submit-routing-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        apply(&app, Msg::Busy(true));
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
        apply(&app, Msg::Busy(true));
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
        start.prefs.splash = true;
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
                app.model.borrow_mut().busy_since = busy.then(Instant::now);
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
        assert_eq!(app.draft.peek(), "/settings");
        assert!(app.complete());
        assert_eq!(app.draft.peek(), "/session");
        assert!(app.complete());
        assert_eq!(app.draft.peek(), "/settings");
    });
}

#[test]
fn slash_model_opens_the_picker_without_starting_a_turn() {
    sigil::reactive::Runtime::scope("mage-model-command-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let mut start = info();
        start.prefs.splash = true;
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
fn slash_popup_shows_the_complete_command_catalog() {
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

        assert!(first.contains("/model"), "model command missing:\n{first}");
        assert!(first.contains("tab accept"), "key hint missing:\n{first}");

        app.shift(-1);
        let tree = app.clone();
        let last = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(last.contains("› /quit"), "catalog did not scroll:\n{last}");
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
        app.toggle_reasoning();

        assert!(app.transcript.peek().contains("settings not saved"));
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
