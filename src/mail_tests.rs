use super::*;

#[test]
fn the_environment_joins_the_mailbox_when_the_module_is_off() {
    assert!(mail_enabled(Some(OsStr::new("on")), false));
    assert!(!mail_enabled(Some(OsStr::new("off")), true));
    assert!(mail_enabled(None, true));
    assert!(!mail_enabled(None, false));
}

#[test]
fn clear_refuses_the_inbox_of_a_live_session() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("agents")).unwrap();
    let agent = Agent {
        id: "s1".to_string(),
        name: String::new(),
        kind: default_kind(),
        model: "model".to_string(),
        cwd: "/work".to_string(),
        pid: 1,
        status: "idle".to_string(),
        last_seen: now(),
    };
    std::fs::write(
        dir.path().join("agents/s1.json"),
        serde_json::to_string(&agent).unwrap(),
    )
    .unwrap();
    let mail = Mail {
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
    )
    .unwrap();
    let mail = receiver.poll().unwrap();
    assert_eq!(mail.len(), 1);
    let shown = envelope(&mail[0]);
    assert!(
        shown.starts_with("[mail from @summonr-1 ") && shown.ends_with("the schema landed"),
        "{shown}"
    );
    drop(sender);
}
