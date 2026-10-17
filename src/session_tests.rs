use super::*;

#[test]
fn a_profile_names_the_latest_nonempty_answering_model() {
    let home = tempfile::tempdir().expect("journal home");
    let path = home.path().join("session.jsonl");
    std::fs::write(&path, concat!(
        "{\"kind\":\"session\",\"id\":\"000000000000000018d2297ad34281e1\",\"model\":\"qwen\",\"system\":\"\",\"workspace\":\"/work\"}\n",
        "{\"kind\":\"assistant\",\"text\":\"first\",\"model\":\"gpt-6.1-sol\",\"usage\":{\"input\":10,\"output\":5,\"cached\":0,\"millis\":20}}\n",
        "{\"kind\":\"assistant\",\"text\":\"legacy\",\"usage\":{\"input\":10,\"output\":5,\"cached\":0,\"millis\":20}}\n"
    )).expect("write model switch journal");
    let report = crate::profile(crate::ProfileScope::Session(&path)).expect("profile session");
    assert!(report.contains("gpt-6.1-sol"), "{report}");
    assert!(!report.contains("qwen"), "{report}");
}

#[test]
fn historical_usage_excludes_estimates_and_keeps_legacy_charges() {
    let home = tempfile::tempdir().expect("journal home");
    let dir =
        crate::durable::log::sessions_dir(home.path()).join("000000000000000018d2297ad34281e1");
    std::fs::create_dir_all(&dir).expect("session directory");
    let usage = |input, reported: &str| {
        format!(
            "{{\"kind\":\"charge\",\"note\":\"side request\",\"model\":\"m\",\"usage\":{{\"input\":{input},\"output\":5,\"cached\":0,\"millis\":20}}{reported}}}\n"
        )
    };
    let head = "{\"kind\":\"session\",\"id\":\"000000000000000018d2297ad34281e1\",\"model\":\"m\",\"system\":\"\",\"workspace\":\"/work\"}\n";
    std::fs::write(
        dir.join("session.jsonl"),
        format!(
            "{head}{}{}{}",
            usage(1000, ",\"reported\":false"),
            usage(20, ",\"reported\":true"),
            usage(30, "")
        ),
    )
    .expect("write charge journal");
    let rows =
        spend(home.path(), &[], &std::collections::BTreeMap::new()).expect("historical usage");
    assert_eq!(rows.iter().map(|row| row.input).sum::<u64>(), 50);
    assert_eq!(rows.iter().map(|row| row.output).sum::<u64>(), 10);
    assert_eq!(rows.iter().map(|row| row.turns).sum::<u64>(), 2);
}

#[test]
fn historical_usage_keeps_the_cost_each_turn_recorded() {
    let home = tempfile::tempdir().expect("journal home");
    let dir =
        crate::durable::log::sessions_dir(home.path()).join("000000000000000018d2297ad34281e1");
    std::fs::create_dir_all(&dir).expect("session directory");
    std::fs::write(dir.join("session.jsonl"), concat!(
        "{\"kind\":\"session\",\"id\":\"000000000000000018d2297ad34281e1\",\"model\":\"m\",\"system\":\"\",\"workspace\":\"/work\"}\n",
        "{\"kind\":\"assistant\",\"text\":\"a\",\"model\":\"m\",\"cost\":0.5,\"usage\":{\"input\":1000,\"output\":5,\"cached\":0,\"millis\":20}}\n",
        "{\"kind\":\"assistant\",\"text\":\"b\",\"model\":\"m\",\"usage\":{\"input\":1000000,\"output\":0,\"cached\":0,\"millis\":20}}\n"
    )).expect("write cost journal");
    let price: crate::provider::Price =
        serde_json::from_value(serde_json::json!({"input": 1.0, "output": 1.0})).unwrap();
    let prices = std::collections::BTreeMap::from([("m".to_string(), price)]);
    let rows = spend(home.path(), &[], &prices).expect("historical usage");
    let cost: f64 = rows.iter().filter_map(|row| row.cost).sum();
    assert!((cost - 1.5).abs() < 1e-9, "{cost}");
}

#[test]
fn user_text_strips_the_harness_note() {
    assert_eq!(
        user_text(
            "hi\n\nHarness state: auto mode. Do only what they asked.",
            None
        ),
        "hi"
    );
}

#[test]
fn sessions_idle_past_the_retention_are_removed_except_the_open_one() {
    let home = tempfile::tempdir().unwrap();
    let sessions = crate::durable::log::sessions_dir(home.path());
    for id in ["old", "open"] {
        std::fs::create_dir_all(sessions.join(id)).unwrap();
        std::fs::write(sessions.join(id).join("session.jsonl"), "{}\n").unwrap();
    }
    let retain = crate::limits::limits().session_retain();
    let now = std::time::SystemTime::now();
    assert_eq!(super::prune::prune_in(home.path(), retain, None, now), 0);
    let later = now + retain + std::time::Duration::from_secs(60);
    assert_eq!(
        super::prune::prune_in(home.path(), retain, Some("open"), later),
        1
    );
    assert!(!sessions.join("old").exists());
    assert!(sessions.join("open").join("session.jsonl").exists());
}

#[test]
fn usage_counts_each_session_once_and_keeps_its_workspace() {
    let home = tempfile::tempdir().unwrap();
    let sessions = crate::durable::log::sessions_dir(home.path());
    let turn = |input: u64, at: u64| {
        format!(
            r#"{{"kind":"assistant","text":"ok","usage":{{"input":{input},"output":5,"cached":{},"millis":2000}},"model":"glm","at":{at}}}"#,
            input / 2
        )
    };
    let logs = [
        (
            "000000000000000018d2297ad34281e1",
            "/work/alpha",
            vec![turn(100, 3_600_000), turn(300, 7_300_000)],
        ),
        (
            "000000000000000018d2297ad34281e2",
            "/work/beta",
            vec![turn(40, 3_600_000)],
        ),
    ];
    for (id, workspace, turns) in logs {
        std::fs::create_dir_all(sessions.join(id)).unwrap();
        let head = format!(
            r#"{{"kind":"session","id":"{id}","model":"glm","system":"","workspace":"{workspace}"}}"#
        );
        let body = std::iter::once(head)
            .chain(turns)
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(sessions.join(id).join("session.jsonl"), body + "\n").unwrap();
    }
    let rows = spend(home.path(), &[], &std::collections::BTreeMap::new()).unwrap();
    let alpha: Vec<_> = rows
        .iter()
        .filter(|row| row.workspace == "/work/alpha")
        .collect();
    assert_eq!(alpha.iter().map(|row| row.sessions).sum::<u64>(), 1);
    assert_eq!(alpha.iter().map(|row| row.turns).sum::<u64>(), 2);
    assert_eq!(alpha.iter().map(|row| row.input).sum::<u64>(), 400);
    assert_eq!(alpha.iter().map(|row| row.millis).sum::<u64>(), 4000);
    assert_eq!(rows.iter().map(|row| row.sessions).sum::<u64>(), 2);
    let latest = alpha.iter().max_by_key(|row| row.hour).unwrap();
    assert_eq!(
        latest.sessions, 1,
        "a session counts where it was last active"
    );
    assert!(
        rows.iter()
            .any(|row| row.workspace == "/work/beta" && row.turns == 1)
    );
}

#[test]
fn steering_note_never_counts_as_the_users_words_in_old_history() {
    let stored = "fix the test\n\nHarness: the previous turn stopped early (cancelled). Address it.\n\nHarness state: auto mode.";
    assert_eq!(super::user_text(stored, None), "fix the test");
}

#[test]
fn a_user_paragraph_that_starts_with_harness_stays_in_the_message() {
    let typed = "Fix this.\n\nHarness the existing API and preserve retries.";
    assert_eq!(user_text(typed, None), typed);
    let noted = format!("{typed}\n\nHarness state: auto mode. Do only what they asked.");
    assert_eq!(user_text(&noted, None), typed);
}

#[test]
fn a_listed_session_is_not_read_again_once_it_has_a_first_prompt() {
    let home = tempfile::tempdir().unwrap();
    let sessions = crate::durable::log::sessions_dir(home.path());
    let id = "000000000000000018d2297ad34281e3";
    let journal = sessions.join(id).join("session.jsonl");
    std::fs::create_dir_all(journal.parent().unwrap()).unwrap();
    std::fs::write(
        &journal,
        format!(
            "{}\n{}\n",
            r#"{"kind":"session","id":"000000000000000018d2297ad34281e3","model":"glm","system":"","workspace":"/work/alpha"}"#,
            r#"{"kind":"user","text":"find the slow startup","start":true}"#
        ),
    )
    .unwrap();
    let first = super::catalog::list_in(home.path()).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].preview, "find the slow startup");
    std::fs::write(&journal, "{}\n").unwrap();
    let again = super::catalog::list_in(home.path()).unwrap();
    assert_eq!(again[0].preview, "find the slow startup");
    assert_eq!(again[0].cwd.as_deref(), Some("/work/alpha"));
}
