use std::env::var_os;
use std::path::PathBuf;

use sigil::prelude::{Cx, Key, Role, signal};
use sigil::reactive::Runtime;
use sigil_tui::{EventRouter, Host, Screen, key_down, paste, present};
use tokio::sync::mpsc::unbounded_channel;

use super::root;
use crate::diff::{Diff, Stat};
use crate::tui::sigil_view::app::App;
use crate::tui::sigil_view::pane::Pane;
use crate::tui::{Cmd, Msg};

fn app() -> (App, tokio::sync::mpsc::UnboundedReceiver<Cmd>) {
    let (cmd, rx) = unbounded_channel();
    let mut info = super::super::tests::info();
    info.prefs.splash = false;
    let mut app = App::new(info, PathBuf::from("/tmp/mage-desktop-test"), cmd);
    app.extent = signal(1.0);
    app.resize(160, 50);
    (app, rx)
}

#[test]
fn desktop_composer_keeps_focus_between_editing_and_submission() {
    Runtime::scope("summoner-desktop-composer-focus", || {
        let (app, mut rx) = app();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(160, 80);
        screen
            .frame(&mut host, &mut router, [], (160, 80))
            .expect("initial frame");
        let root = host.root().expect("tree root");
        let mut editor = None;
        host.cx().visit_preorder(root, |id, node| {
            if node.a11y.role == Role::TextInput {
                editor = Some(id);
            }
        });
        let editor = editor.expect("composer input");
        router.set_focus(host.cx(), Some(editor));
        screen
            .frame(
                &mut host,
                &mut router,
                [paste("Describe fixture")],
                (160, 80),
            )
            .expect("edited frame");
        assert_eq!(app.draft.peek(), "Describe fixture");
        app.update(Msg::Peers(0));
        app.refresh();
        screen
            .frame(&mut host, &mut router, [], (160, 80))
            .expect("next frame");
        assert_eq!(
            router.focus(),
            Some(editor),
            "editing must retain the field"
        );
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
        screen
            .frame(&mut host, &mut router, [key_down(Key::Enter)], (160, 80))
            .expect("submitted frame");
        assert!(
            matches!(rx.try_recv(), Ok(Cmd::Submit(input)) if input.text == "Describe fixture")
        );
    });
}

#[test]
fn desktop_authentication_routes_masked_input_save_cancel_and_device_login() {
    Runtime::scope("summoner-desktop-authentication", || {
        let (app, mut rx) = app();
        app.replace("/auth glm".to_string());
        app.submit();
        app.replace("keep this draft".to_string());
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 60);
        let providers = screen
            .frame(&mut host, &mut router, [], (120, 60))
            .expect("provider form")
            .text();
        assert!(providers.contains("Authentication"), "{providers}");
        assert!(providers.contains("glm"), "{providers}");
        assert!(providers.contains("Sign in"), "{providers}");
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
        let form = screen
            .frame(&mut host, &mut router, [key_down(Key::Enter)], (120, 60))
            .expect("API key form")
            .text();
        assert!(app.auth_entering.peek());
        assert!(form.contains("credential store"), "{form}");
        let masked = screen
            .frame(&mut host, &mut router, [paste("fixture-secret")], (120, 60))
            .expect("masked key")
            .text();
        assert_eq!(app.auth_key.peek(), "fixture-secret");
        assert_eq!(
            host.cx()
                .get(router.focus().expect("field focus"))
                .expect("field")
                .a11y
                .role,
            Role::PasswordInput
        );
        assert!(!masked.contains("fixture-secret"), "{masked}");
        assert!(masked.contains("**************"), "{masked}");
        if var_os("MAGE_DUMP_FRAME").is_some() {
            println!(
                "desktop providers 120x60:\n{providers}\ndesktop masked key 120x60:\n{masked}"
            );
        }
        screen
            .frame(&mut host, &mut router, [key_down(Key::Enter)], (120, 60))
            .expect("save key");
        assert!(
            matches!(rx.try_recv(), Ok(Cmd::SetKey(name, key)) if name == "glm" && key == "fixture-secret")
        );
        assert!(!app.auth_open.peek());
        assert!(app.auth_key.peek().is_empty());
        assert_eq!(app.draft.peek(), "keep this draft");

        app.begin_auth(Some("glm"));
        screen
            .frame(&mut host, &mut router, [], (120, 60))
            .expect("reopen");
        screen
            .frame(&mut host, &mut router, [key_down(Key::Enter)], (120, 60))
            .expect("new key");
        screen
            .frame(
                &mut host,
                &mut router,
                [
                    paste("discard-this-key"),
                    key_down(Key::Tab),
                    key_down(Key::Tab),
                ],
                (120, 60),
            )
            .expect("focus close");
        screen
            .frame(&mut host, &mut router, [key_down(Key::Enter)], (120, 60))
            .expect("close key entry");
        assert!(app.auth_open.peek());
        assert!(!app.auth_entering.peek());
        assert!(app.auth_key.peek().is_empty());
        assert!(rx.try_recv().is_err(), "Close must not save a key");
        screen
            .frame(&mut host, &mut router, [key_down(Key::Escape)], (120, 60))
            .expect("close providers");
        assert!(!app.auth_open.peek());

        app.begin_auth(Some("copilot"));
        screen
            .frame(&mut host, &mut router, [], (120, 60))
            .expect("device provider");
        screen
            .frame(&mut host, &mut router, [key_down(Key::Enter)], (120, 60))
            .expect("start device sign-in");
        assert!(matches!(rx.try_recv(), Ok(Cmd::Login(name)) if name == "copilot"));
        assert!(!app.auth_open.peek());
        assert_eq!(app.draft.peek(), "keep this draft");
        assert!(rx.try_recv().is_err());
    });
}

#[test]
fn desktop_options_explain_persistence_and_model_identity() {
    Runtime::scope("summoner-desktop-option-cues", || {
        let (app, mut rx) = app();
        app.toggle_options();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(160, 100);
        let options = screen
            .frame(&mut host, &mut router, [], (160, 100))
            .expect("settings")
            .text();
        assert!(options.contains("Session · not saved"), "{options}");
        assert!(options.contains("Provider · not saved"), "{options}");
        assert!(options.contains("Interface · auto-save"), "{options}");
        {
            let mut model = app.model.borrow_mut();
            model.snapshot.endpoint = "alpha".to_string();
            model.snapshot.model = "shared".to_string();
            model.model = "shared".to_string();
        }
        app.begin_model_edit();
        app.model_choices.set(vec![
            "alpha\tshared".to_string(),
            "beta\tshared".to_string(),
        ]);
        app.model_loading.set(false);
        app.model_sel.set(1);
        let models = screen
            .frame(&mut host, &mut router, [], (160, 100))
            .expect("models")
            .text();
        assert!(models.contains("alpha · shared · current"), "{models}");
        assert!(!models.contains("beta · shared · current"), "{models}");
        assert_eq!(models.matches("· current").count(), 1);
        app.replace_model_draft("unlisted-model-id".to_string());
        let empty = screen
            .frame(&mut host, &mut router, [], (160, 100))
            .expect("exact ID hint")
            .text();
        assert!(empty.contains("Enter uses this exact model ID."), "{empty}");
        if var_os("MAGE_DUMP_FRAME").is_some() {
            println!(
                "desktop options 160x100:\n{options}\ndesktop models 160x100:\n{models}\ndesktop exact ID 160x100:\n{empty}"
            );
        }
        screen
            .frame(&mut host, &mut router, [key_down(Key::Enter)], (160, 100))
            .expect("choose exact ID");
        assert!(!app.editing_model.peek());
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetModel(name)) if name == "unlisted-model-id"));
        assert!(rx.try_recv().is_err());
    });
}

#[test]
fn saved_prompt_selection_stays_visible_in_a_short_desktop_window() {
    for branch in [false, true] {
        sigil::reactive::Runtime::scope("summoner-desktop-prompts", || {
            let (app, mut rx) = app();
            if branch {
                app.replace("/branch".to_string());
                app.submit();
            } else {
                assert!(app.route_hotkey('r'));
            }
            app.replace("prefix  suffix".to_string());
            app.caret.set(7);
            assert!(
                matches!(rx.try_recv(), Ok(Cmd::ListPrompts(scope)) if scope.is_some() == branch)
            );
            let session = std::sync::Arc::new(crate::session::SessionInfo {
                id: app.model.borrow().session.clone(),
                path: PathBuf::from("saved-session"),
                cwd: Some("/another-project".to_string()),
                model: Some("test-model".to_string()),
                preview: String::new(),
                name: Some("Parser history".to_string()),
                parent: None,
                depth: 0,
            });
            app.update(Msg::Prompts {
                session: branch.then(|| session.id.clone()),
                result: Ok((
                    (0..40)
                        .map(|turn| crate::session::Prompt {
                            session: session.clone(),
                            turn,
                            text: format!("Past prompt {turn}"),
                            images: 0,
                            closed: true,
                        })
                        .collect(),
                    Vec::new(),
                )),
            });
            let tree = app.clone();
            let mut host = Host::cells();
            host.mount(move |cx: &mut Cx| cx.add(root(tree)));
            let mut router = EventRouter::new();
            let mut screen = Screen::new(120, 50);
            screen
                .frame(&mut host, &mut router, [], (120, 50))
                .expect("prompt search");
            router.focus_next(host.cx());
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [
                        sigil_tui::key_down(Key::PageDown),
                        sigil_tui::key_down(Key::PageDown),
                    ],
                    (120, 50),
                )
                .expect("later prompts");
            let frame = screen
                .frame(&mut host, &mut router, [], (120, 50))
                .expect("measured results");
            assert!(frame.text().contains("Past prompt 16"), "{}", frame.text());
            assert!(frame.text().contains("Close search"), "{}", frame.text());
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::key_down(Key::Enter)],
                    (120, 50),
                )
                .expect("insert selected text");
            assert!(!app.search.open.peek());
            assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
            if branch {
                assert_eq!(app.draft.peek(), "prefix  suffix");
                assert!(matches!(rx.try_recv(), Ok(Cmd::Branch { turn: 16, .. })));
            } else {
                assert_eq!(app.draft.peek(), "prefix Past prompt 16 suffix");
            }
            assert!(rx.try_recv().is_err());
        });
    }
}

#[test]
fn desktop_session_tree_shows_the_current_branch_and_selects_its_parent() {
    sigil::reactive::Runtime::scope("summoner-desktop-tree", || {
        let (app, mut rx) = app();
        let current = app.model.borrow().session.clone();
        app.replace("/tree".to_string());
        app.submit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListSessions)));
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
            session(&current, "This branch", Some("parent")),
            session("parent", "Original task", None),
            session("other", "Unrelated", None),
        ])));
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 50);
        screen
            .frame(&mut host, &mut router, [], (120, 50))
            .expect("session tree");
        let frame = screen
            .frame(&mut host, &mut router, [], (120, 50))
            .expect("measured tree");
        assert!(frame.text().contains("Session tree"), "{}", frame.text());
        assert!(frame.text().contains("Original task"), "{}", frame.text());
        assert!(frame.text().contains("current"), "{}", frame.text());
        assert!(frame.text().contains("Close sessions"), "{}", frame.text());
        assert!(!frame.text().contains("Unrelated"));
        router.focus_next(host.cx());
        screen
            .frame(
                &mut host,
                &mut router,
                [
                    sigil_tui::key_down(Key::ArrowUp),
                    sigil_tui::key_down(Key::Enter),
                ],
                (120, 50),
            )
            .expect("select parent");
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
        assert!(matches!(rx.try_recv(), Ok(Cmd::Command(command)) if command == "/resume parent"));
    });
}

#[test]
fn search_uses_desktop_message_bounds_and_keeps_the_draft() {
    sigil::reactive::Runtime::scope("summoner-desktop-search", || {
        let (app, mut rx) = app();
        for index in 0..40 {
            app.update(Msg::User(format!(
                "Message {index}: {}",
                "conversation text ".repeat(20)
            )));
        }
        app.replace("keep the desktop draft".to_string());
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(160, 80);
        screen
            .frame(&mut host, &mut router, [], (160, 80))
            .expect("desktop");
        for query in ["Message 3:", "Message 35:"] {
            app.search.start(query);
            let frame = screen
                .frame(&mut host, &mut router, [], (160, 80))
                .expect("search");
            assert!(
                frame.text().contains("Search conversation"),
                "{}",
                frame.text()
            );
            router.focus_next(host.cx());
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::key_down(Key::Enter)],
                    (160, 80),
                )
                .expect("jump");
            let frame = screen
                .frame(&mut host, &mut router, [], (160, 80))
                .expect("measured jump");
            assert!(!app.search.open.peek());
            assert_eq!(app.search.target.peek(), None);
            assert!(app.scroll_y.peek() > 0.0);
            assert!(frame.text().contains(query), "{}", frame.text());
            assert!(
                !frame.text().contains("Message 0:"),
                "jump must scroll older messages away"
            );
        }
        assert_eq!(app.draft.peek(), "keep the desktop draft");
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
        assert!(rx.try_recv().is_err());
    });
}

#[test]
fn the_window_shows_the_session_and_asks_for_the_diff() {
    Runtime::scope("summoner-desktop-tree", || {
        let (app, mut rx) = app();
        let frame = present(move |cx: &mut Cx| cx.add(root(app)), 160, 80, &[]).expect("desktop");
        assert!(frame.contains("session-12345"), "{frame}");
        assert!(frame.contains("ask for changes"), "{frame}");
        assert!(matches!(rx.try_recv(), Ok(Cmd::Diff(None))));
    });
}

#[test]
fn the_right_rail_follows_the_open_pane() {
    Runtime::scope("summoner-desktop-panes", || {
        let (app, mut rx) = app();
        app.update(Msg::Diff(Diff {
            base: "HEAD".to_string(),
            files: vec![Stat {
                path: "src/lib.rs".to_string(),
                added: 2,
                removed: 1,
                new: false,
            }],
        }));
        app.refresh();
        for (pane, expected) in [
            (Pane::Diff, "src/lib.rs"),
            (Pane::Files, "no files listed yet"),
            (Pane::Worktrees, "no worktrees"),
            (Pane::Ledger, "no plan is running"),
        ] {
            app.toggle_pane(pane);
            assert_eq!(app.layout.peek().secondary, Some(pane));
            let tree = app.clone();
            let frame = present(move |cx: &mut Cx| cx.add(root(tree)), 160, 80, &[]).expect("pane");
            assert!(frame.contains(expected), "{pane:?}: {frame}");
            app.toggle_pane(pane);
        }
        app.toggle_pane(Pane::Diff);
        app.diff_open("src/lib.rs".to_string());
        app.update(Msg::Patch {
            path: "src/lib.rs".to_string(),
            text: "@@ -1 +1,2 @@\n-a\n+b\n+c\n".to_string(),
        });
        app.refresh();
        let tree = app.clone();
        let frame = present(move |cx: &mut Cx| cx.add(root(tree)), 160, 80, &[]).expect("hunk");
        assert!(frame.contains("@@ -1 +1,2 @@"), "{frame}");
        assert!(frame.contains("+b"), "{frame}");
        while rx.try_recv().is_ok() {}
    });
}

#[test]
fn the_desktop_hotkey_editor_shows_a_changed_binding() {
    Runtime::scope("summoner-desktop-options", || {
        let (app, _rx) = app();
        app.toggle_options();
        assert!(app.settings_open.peek());

        app.settings_sel.set(12);
        app.adjust_setting(1);
        assert!(app.hotkeys_open.peek());

        app.edit_hotkey();
        assert!(app.route_hotkey('k'));
        assert_eq!(
            app.model
                .borrow()
                .prefs
                .hotkeys
                .label(crate::prefs::Hotkey::Options),
            "ctrl+k"
        );
        let tree = app.clone();
        let frame = present(move |cx: &mut Cx| cx.add(root(tree)), 160, 80, &[]).expect("hotkeys");
        assert!(frame.contains("Hotkeys"), "{frame}");
        assert!(frame.contains("ctrl+k"), "{frame}");
    });
}
