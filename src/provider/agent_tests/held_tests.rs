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

#[tokio::test(flavor = "multi_thread")]
async fn output_over_turns_is_capped_per_request() {
    let dir = tempfile::tempdir().unwrap();
    let wide = "  *'\"type\":\"user\"'*)\n    big=$(head -c 5000000 /dev/zero | tr '\\0' x)\n    printf '{\"type\":\"stream_event\",\"event\":{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"%s\"}}}\\n' \"$big\"\n";
    let script = PLAIN.replacen("  *'\"type\":\"user\"'*)\n", wide, 1);
    let client = seat_at(dir.path(), &script);
    let mut messages = vec![Message::system("sys"), Message::user("first")];
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();
    reply(&mut messages, "second");
    let second = client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await;
    assert!(second.is_ok(), "{:?}", second.err());
    assert_eq!(runs(dir.path()).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_effort_starts_a_new_claude_process_and_the_same_effort_reuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let client = seat_at(dir.path(), PLAIN);
    let mut messages = vec![Message::system("sys"), Message::user("first")];
    let mut low = ask(&messages, &[]);
    low.effort = Some(crate::provider::Effort::Low);
    client.stream(&low, &mut Partial::default()).await.unwrap();
    reply(&mut messages, "second");
    let mut same = ask(&messages, &[]);
    same.effort = Some(crate::provider::Effort::Low);
    client.stream(&same, &mut Partial::default()).await.unwrap();
    assert_eq!(
        runs(dir.path()).len(),
        1,
        "the same effort restarted claude"
    );
    reply(&mut messages, "third");
    let mut high = ask(&messages, &[]);
    high.effort = Some(crate::provider::Effort::High);
    client.stream(&high, &mut Partial::default()).await.unwrap();
    assert_eq!(
        runs(dir.path()).len(),
        2,
        "a new effort kept the old claude"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_earlier_message_that_changes_its_image_starts_a_new_claude_process() {
    let dir = tempfile::tempdir().unwrap();
    let client = seat_at(dir.path(), PLAIN);
    let mut first = Message::user("look at this");
    first
        .images
        .push(crate::provider::Image::encode("image/png", b"one"));
    let mut messages = vec![Message::system("sys"), first];
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();
    reply(&mut messages, "second");
    messages[1].images[0] = crate::provider::Image::encode("image/png", b"two");
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();
    assert_eq!(
        runs(dir.path()).len(),
        2,
        "a changed earlier image kept the old process"
    );
}
