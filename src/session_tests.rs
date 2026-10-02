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
