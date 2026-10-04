use super::*;

#[test]
fn user_text_strips_the_harness_note() {
    assert_eq!(
        user_text("hi\n\nHarness state: auto mode. Do only what they asked."),
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
    let retain = crate::limits::SESSION_RETAIN;
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
