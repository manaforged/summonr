use super::*;

#[test]
fn the_environment_joins_the_mailbox_when_the_module_is_off() {
    assert!(mail_enabled(Some(OsStr::new("on")), false));
    assert!(!mail_enabled(Some(OsStr::new("off")), true));
    assert!(mail_enabled(None, true));
    assert!(!mail_enabled(None, false));
}

#[test]
fn an_unresolved_mail_name_fails_and_writes_no_inbox() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = std::path::Path::new("/work/summonr");
    let _sender = Mailbox::open(dir.path(), "s1", Some("sender"), "summonr", "model", cwd).unwrap();
    let error = deliver_in(dir.path(), "s1", &["missing".to_string()], "hello", None).unwrap_err();
    assert!(error.to_string().contains("missing"), "{error}");
    assert!(!dir.path().join("inbox/missing.jsonl").exists());
}

#[test]
fn clear_refuses_the_inbox_of_a_live_session() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("agents")).unwrap();
    let agent: Agent = serde_json::from_value(serde_json::json!({
        "id": "s1", "model": "model", "cwd": "/work", "pid": 1,
        "status": "idle", "last_seen": now()
    }))
    .expect("peer record");
    std::fs::write(
        dir.path().join("agents/s1.json"),
        serde_json::to_string(&agent).unwrap(),
    )
    .unwrap();
    let mail = Mail {
        id: "fixture-message".to_string(),
        to: "s1".to_string(),
        reply_to: None,
        ts: now(),
        from: "peer".to_string(),
        name: String::new(),
        kind: default_kind(),
        text: "hello".to_string(),
    };
    append(dir.path(), &["s1".to_string()], &mail).unwrap();
    let error = take_inbox(dir.path(), "s1").unwrap_err();
    assert!(error.to_string().contains("live session"), "{error}");
    assert_eq!(read_inbox(dir.path(), "s1").unwrap(), vec![mail]);
}

#[test]
fn sessions_in_one_workspace_get_distinct_names() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = Path::new("/work/summonr");
    let first = Mailbox::open(dir.path(), "s1", None, "summonr", "model", cwd).unwrap();
    let second = Mailbox::open(dir.path(), "s2", None, "summonr", "model", cwd).unwrap();
    let named = Mailbox::open(dir.path(), "s3", Some("review"), "summonr", "model", cwd).unwrap();
    let names: Vec<String> = live(dir.path())
        .into_iter()
        .map(|agent| agent.name)
        .collect();
    assert_eq!(names, ["summonr-1", "summonr-2", "review"]);
    drop((first, second, named));
}

#[test]
fn a_running_tool_stays_off_the_parent_inbox() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = Path::new("/work/summonr");
    let mut child =
        Mailbox::open(dir.path(), "child", Some("child"), "summonr", "model", cwd).unwrap();
    let _parent = Mailbox::open(
        dir.path(),
        "parent",
        Some("parent"),
        "summonr",
        "model",
        cwd,
    )
    .unwrap();
    child.note_tool("model", cwd, ("shell", "{}"), Some("parent"));
    std::thread::park_timeout(std::time::Duration::from_millis(50));
    let mail = read_inbox(dir.path(), "parent").unwrap();
    assert!(mail.is_empty(), "{mail:?}");
    let record: Agent = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("agents/child.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record.tool, "shell");
}

#[test]
fn mail_to_a_name_arrives_under_the_senders_name() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = Path::new("/work/summonr");
    let sender = Mailbox::open(dir.path(), "s1", None, "summonr", "model", cwd).unwrap();
    let mut receiver = Mailbox::open(dir.path(), "s2", None, "summonr", "model", cwd).unwrap();
    deliver_in(
        dir.path(),
        "s1",
        &["summonr-2".to_string()],
        "the schema landed",
        None,
    )
    .unwrap();
    let mail = receiver.poll().unwrap();
    assert_eq!(mail.len(), 1);
    assert!(!mail[0].id.is_empty());
    assert_eq!(mail[0].from, "s1");
    assert_eq!(mail[0].to, "s2");
    let sent = std::fs::read_to_string(files::audit_path(dir.path(), "s1")).expect("sender audit");
    let received =
        std::fs::read_to_string(files::audit_path(dir.path(), "s2")).expect("receiver audit");
    assert!(sent.contains("send_attempted"));
    assert!(sent.contains("delivered"));
    assert!(received.contains("dequeued"));
    assert!(sent.contains(&mail[0].id));
    assert!(received.contains(&mail[0].id));
    let shown = envelope(&mail[0]);
    assert!(
        shown.starts_with("[mail from @summonr-1 ") && shown.ends_with("the schema landed"),
        "{shown}"
    );
    drop(sender);
}

#[test]
fn a_held_inbox_lock_does_not_hide_delivered_mail() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = Path::new("/work/summonr");
    let _sender = Mailbox::open(dir.path(), "s1", None, "summonr", "model", cwd).unwrap();
    let mut receiver = Mailbox::open(dir.path(), "s2", None, "summonr", "model", cwd).unwrap();
    deliver_in(
        dir.path(),
        "s1",
        &["summonr-2".to_string()],
        "hold on",
        None,
    )
    .unwrap();
    let held = crate::durable::store::lock(&inbox_path(dir.path(), "s2"), true).unwrap();
    assert_eq!(receiver.poll().unwrap().len(), 1);
    drop(held);
    assert!(
        receiver.poll().unwrap().is_empty(),
        "an unchanged inbox is not reread"
    );
}

#[cfg(unix)]
#[test]
fn mail_arrives_while_other_threads_start_processes() {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let spawners: Vec<_> = (0..4)
        .map(|_| {
            let stop = std::sync::Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    drop(std::process::Command::new("true").status());
                }
            })
        })
        .collect();
    let cwd = Path::new("/work/summonr");
    let missed = (0..100)
        .filter(|_| {
            let dir = tempfile::tempdir().unwrap();
            let _sender = Mailbox::open(dir.path(), "s1", None, "summonr", "model", cwd).unwrap();
            let mut receiver =
                Mailbox::open(dir.path(), "s2", None, "summonr", "model", cwd).unwrap();
            deliver_in(dir.path(), "s1", &["summonr-2".to_string()], "ping", None).unwrap();
            receiver.poll().unwrap().is_empty()
        })
        .count();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    spawners
        .into_iter()
        .for_each(|spawner| spawner.join().unwrap());
    assert_eq!(missed, 0);
}

#[test]
fn the_board_keeps_the_latest_value_per_key_in_first_post_order() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = std::path::Path::new("/work/board");
    super::post_board(dir.path(), cwd, "a", "k", "v1").unwrap();
    super::post_board(dir.path(), cwd, "b", "k", "v2").unwrap();
    super::post_board(dir.path(), cwd, "a", "j", "x").unwrap();
    let board = super::read_board(dir.path(), cwd).unwrap();
    let owned =
        |key: &str, value: &str, from: &str| (key.to_string(), value.to_string(), from.to_string());
    assert_eq!(board, vec![owned("k", "v2", "b"), owned("j", "x", "a")]);
}

#[test]
fn a_long_board_folds_to_one_line_per_key_and_a_short_one_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = std::path::Path::new("/work/fold");
    let file = |dir: &std::path::Path| {
        std::fs::read_dir(dir.join("board"))
            .unwrap()
            .flatten()
            .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
            .next()
            .unwrap_or_default()
    };
    super::post_board(dir.path(), cwd, "a", "k", "v1").unwrap();
    super::post_board(dir.path(), cwd, "a", "j", "x").unwrap();
    super::read_board(dir.path(), cwd).unwrap();
    assert_eq!(file(dir.path()).lines().count(), 2);
    let value = "v".repeat(256);
    for _ in 0..=super::BOARD_COMPACT_BYTES / value.len() {
        super::post_board(dir.path(), cwd, "a", "k", &value).unwrap();
    }
    let board = super::read_board(dir.path(), cwd).unwrap();
    assert_eq!(board.len(), 2);
    assert_eq!(file(dir.path()).lines().count(), 2);
}

#[test]
fn a_partial_inbox_line_is_not_pending_until_its_newline_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = Path::new("/work");
    let mut mailbox = Mailbox::open(dir.path(), "s1", None, "summonr", "model", cwd).unwrap();
    let mail = Mail {
        id: "fixture-message".to_string(),
        to: "s1".to_string(),
        reply_to: None,
        ts: now(),
        from: "peer".to_string(),
        name: String::new(),
        kind: default_kind(),
        text: "hello".to_string(),
    };
    let line = serde_json::to_string(&mail).unwrap();
    let (head, tail) = line.split_at(line.len() / 2);
    let path = inbox_path(dir.path(), "s1");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, head).unwrap();
    assert!(!mailbox.pending().unwrap());
    assert!(mailbox.poll().unwrap().is_empty());
    assert!(!mailbox.pending().unwrap());
    std::fs::write(&path, format!("{head}{tail}\n")).unwrap();
    assert!(mailbox.pending().unwrap());
    assert_eq!(mailbox.poll().unwrap(), vec![mail]);
    assert!(!mailbox.pending().unwrap());
}
