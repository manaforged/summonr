use super::{State, busy_info, frame_with};

#[test]
fn reopening_hidden_slash_completion_refreshes_the_offers() {
    use super::super::app::App;
    use crate::tui::Cmd;
    sigil::reactive::Runtime::scope("skill-reopen", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        assert!(matches!(sent.try_recv(), Ok(Cmd::Offers)));
        app.replace("/sett".to_owned());
        assert!(matches!(sent.try_recv(), Ok(Cmd::Offers)));
        assert!(app.commands.hide("/sett", 5, &app.model.get()));
        app.replace("/setti".to_owned());
        assert!(matches!(sent.try_recv(), Ok(Cmd::Offers)));
        app.replace("/settin".to_owned());
        assert!(sent.try_recv().is_err());
    });
}

#[test]
fn skill_typeahead_matches_fuzzy_names() {
    let shown = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(crate::tui::Msg::Offers(vec![crate::mcp::Offer {
            kind: crate::mcp::OfferKind::Prompt,
            insert: "/probe-custom-skill".to_owned(),
            about: "custom skill".to_owned(),
        }]));
        app.replace("/pcs".to_owned());
    })
    .join("\n");
    assert!(shown.contains("/probe-custom-skill"), "{shown}");
}

#[test]
fn submitting_a_skill_passes_its_arguments_to_the_worker() {
    use super::super::app::App;
    use crate::tui::Cmd;
    sigil::reactive::Runtime::scope("skill-submit", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let skill = workspace.path().join(".claude/skills/probe-ui");
        std::fs::create_dir_all(&skill).expect("skill directory");
        std::fs::write(skill.join("SKILL.md"), "Use the skill.").expect("skill body");
        let (commands, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.replace("/probe-ui inspect the parser".to_owned());
        app.submit();
        let submitted = std::iter::from_fn(|| sent.try_recv().ok())
            .find_map(|command| match command {
                Cmd::Submit(input) => Some(input),
                _ => None,
            })
            .expect("a skill invocation reaches the worker");
        assert_eq!(submitted.text, "/probe-ui inspect the parser");
    });
}

#[test]
fn commands_suggest_and_complete_while_a_turn_runs() {
    let mut completed = String::new();
    let shown = frame_with(busy_info(), 110, 30, State::Busy, |app| {
        app.replace("/sett".to_owned());
        assert!(app.complete());
        completed = app.draft.peek();
        app.replace("/sett".to_owned());
    })
    .join("\n");
    assert!(completed.starts_with("/settings"), "{completed:?}");
    assert!(
        shown.contains("commands") && shown.contains("/settings"),
        "{shown}"
    );
}

#[test]
fn slash_typeahead_shows_command_names_and_syntax_without_descriptions() {
    for (query, label, description) in [
        ("/model", "/model [NAME] [LEVEL]", "choose the model"),
        ("/stats", "/stats-report", "plugin prose"),
    ] {
        let shown = frame_with(busy_info(), 110, 30, State::Idle, |app| {
            app.model_mut(|model| {
                model.offers = vec![crate::mcp::Offer {
                    kind: crate::mcp::OfferKind::Prompt,
                    insert: "/stats-report".to_string(),
                    about: "plugin prose".to_string(),
                }];
            });
            app.replace(query.to_string());
        })
        .join("\n");
        assert!(shown.contains(label), "{shown}");
        assert!(!shown.contains(description), "{shown}");
    }
}

#[test]
fn mention_typeahead_keeps_peer_details() {
    let shown = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(crate::tui::Msg::Peers(vec![crate::mail::Agent {
            id: "0123456789abcdef0123456789abcdef".to_string(),
            name: "build-peer".to_string(),
            kind: crate::mail::KIND.to_string(),
            model: "peer-model".to_string(),
            cwd: "/work/summonr".to_string(),
            pid: 1,
            status: crate::mail::Presence::Idle,
            last_seen: 0,
        }]));
        app.replace("@build".to_string());
    })
    .join("\n");
    let row = shown
        .lines()
        .find(|line| line.contains("build-peer"))
        .expect("peer suggestion");
    assert!(
        row.contains("peer-model") && row.contains("idle"),
        "{shown}"
    );
}

#[test]
fn steering_enter_mid_turn_sends_the_message_into_the_running_turn() {
    use super::super::app::App;
    use crate::tui::{Activity, Cmd, Msg};
    sigil::reactive::Runtime::scope("steer-now", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.replace("stop, check the tests instead".to_owned());
        app.submit();
        let sent: Vec<Cmd> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
        assert!(
            matches!(sent.as_slice(), [.., Cmd::Steer(input)] if input.text == "stop, check the tests instead"),
            "{} commands sent",
            sent.len()
        );
    });
}

#[test]
fn steering_waits_in_the_queue_while_an_approval_is_open() {
    use super::super::app::App;
    use crate::tui::{Activity, Cmd, Msg};
    sigil::reactive::Runtime::scope("steer-approval", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.update(Msg::Event(crate::Event::Approval {
            detail: Some("rm -rf build".to_owned()),
            persist: false,
            dialog: 1,
        }));
        app.replace("yes".to_owned());
        app.submit();
        let sent: Vec<Cmd> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
        assert!(
            matches!(sent.as_slice(), [.., Cmd::Queue(input)] if input.text == "yes"),
            "{} commands sent",
            sent.len()
        );
    });
}

#[test]
fn enter_on_an_empty_composer_never_answers_a_pending_approval() {
    use super::super::app::App;
    use crate::tui::{Activity, Cmd, Msg};
    sigil::reactive::Runtime::scope("queue-approval", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.replace("also check the tests".to_owned());
        app.submit();
        app.update(Msg::Queued {
            steering: 0,
            queued: 1,
        });
        app.update(Msg::Event(crate::Event::Approval {
            detail: Some("rm -rf build".to_owned()),
            persist: false,
            dialog: 1,
        }));
        app.submit();
        let steered = std::iter::from_fn(|| sent.try_recv().ok())
            .filter(|command| matches!(command, Cmd::SteerQueued))
            .count();
        assert_eq!(steered, 0);
    });
}

#[test]
fn messages_sent_into_the_turn_no_longer_count_as_queued() {
    use crate::tui::Msg;
    let shown = frame_with(busy_info(), 110, 30, State::Busy, |app| {
        app.update(Msg::Queued {
            steering: 1,
            queued: 0,
        });
    })
    .join("\n");
    assert!(!shown.contains("queued"), "{shown}");
}

#[test]
fn up_brings_back_a_draft_cleared_with_ctrl_c() {
    let mut seen = (String::new(), String::new());
    let _ = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.replace("half written thought".to_owned());
        app.interrupt();
        let cleared = app.draft.peek();
        assert!(app.recall(true));
        seen = (cleared, app.draft.peek());
    });
    assert_eq!(seen, (String::new(), "half written thought".to_owned()));
}

#[test]
fn enter_on_an_empty_composer_sends_queued_messages_into_the_turn() {
    use super::super::app::App;
    use crate::tui::{Activity, Cmd, Msg};
    sigil::reactive::Runtime::scope("queue-steer", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.update(Msg::Queued {
            steering: 0,
            queued: 1,
        });
        app.submit();
        let steered = std::iter::from_fn(|| sent.try_recv().ok())
            .filter(|command| matches!(command, Cmd::SteerQueued))
            .count();
        assert_eq!(steered, 1);
    });
}
