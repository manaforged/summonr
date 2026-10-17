use super::super::super::{Message, Partial, Provider};
use super::{PLAIN, ask, reply, runs, seat_at};

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_model_starts_a_new_claude_process_that_resumes_the_thread() {
    let dir = tempfile::tempdir().unwrap();
    let client = seat_at(dir.path(), PLAIN);
    let mut messages = vec![Message::system("sys"), Message::user("first")];
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();
    reply(&mut messages, "second");
    let mut other = ask(&messages, &[]);
    other.model = "claude-sonnet-5-5".to_string();
    client
        .stream(&other, &mut Partial::default())
        .await
        .unwrap();

    let runs = runs(dir.path());
    assert_eq!(runs.len(), 2, "{runs:?}");
    let value = |run: &str, flag: &str| {
        let lines: Vec<&str> = run.lines().collect();
        lines
            .windows(2)
            .find(|window| window[0] == flag)
            .map(|window| window[1].to_string())
    };
    let started = value(&runs[0], "--session-id").expect("first run starts a session");
    assert_eq!(value(&runs[1], "--resume"), Some(started));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_claude_process_stops_with_its_client() {
    let dir = tempfile::tempdir().unwrap();
    let client = seat_at(dir.path(), PLAIN);
    let messages = vec![Message::system("sys"), Message::user("first")];
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();
    let pid = std::fs::read_to_string(dir.path().join("claude.pids")).unwrap();
    let alive = |pid: &str| {
        std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .status()
            .is_ok_and(|status| status.success())
    };
    assert!(alive(&pid), "the finished turn did not keep claude");
    drop(client);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while alive(&pid) && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(!alive(&pid), "claude outlived its client");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_turn_forgets_the_claude_thread() {
    let dir = tempfile::tempdir().unwrap();
    let script = PLAIN.replace("HANG_ON", "second");
    let client = seat_at(dir.path(), &script);
    let mut messages = vec![Message::system("sys"), Message::user("first")];
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();
    reply(&mut messages, "second");
    let cancelled = tokio::time::timeout(
        std::time::Duration::from_millis(800),
        client.stream(&ask(&messages, &[]), &mut Partial::default()),
    )
    .await;
    assert!(cancelled.is_err(), "the hanging turn finished");
    messages.push(Message::user("third"));
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();
    let runs = runs(dir.path());
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!(
        !runs[1].contains("--resume"),
        "the cancelled thread was resumed: {}",
        runs[1]
    );
}
