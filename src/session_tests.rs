use super::*;

#[test]
fn replay_rebuilds_user_tools_and_assistant() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "mage-session-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| Error::Config(error.to_string()))?
            .as_nanos()
    ));
    create_dir_all(&dir)?;
    let path = dir.join("s.jsonl");
    let mut log = SessionLog::open(&path)?;
    log.append_user("read it")?;
    log.append_reasoning("need the file first")?;
    log.append_event(&Event::ToolCall {
        id: "c1".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"n"}"#.to_string(),
    })?;
    log.append_event(&Event::ToolResult {
        id: "c1".to_string(),
        success: true,
        mutated: false,
        task: None,
        output: "1:abcd|hi".to_string(),
    })?;
    log.append_event(&Event::Usage {
        prompt_tokens: 120,
        completion_tokens: 30,
        reasoning_tokens: 20,
        cached_tokens: 80,
        prefix_hash: "abc123".to_string(),
    })?;
    log.append_reasoning("kept final thought")?;
    log.append_event(&Event::Assistant {
        text: "done".to_string(),
    })?;
    let replayed = replay(&path)?;
    assert_eq!(replayed.history.len(), 4);
    assert_eq!(replayed.history[0].role, Role::User);
    assert_eq!(replayed.history[1].role, Role::Assistant);
    assert_eq!(replayed.history[1].tool_calls.len(), 1);
    assert_eq!(
        replayed.history[1].reasoning.as_deref(),
        Some("need the file first")
    );
    assert_eq!(replayed.history[2].role, Role::Tool);
    assert_eq!(replayed.history[3].content, "done");
    assert_eq!(
        replayed.history[3].reasoning.as_deref(),
        Some("kept final thought")
    );
    assert_eq!(replayed.spent_tokens, 150);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn replay_discards_an_incomplete_tool_group() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "mage-session-partial-tools-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| Error::Config(error.to_string()))?
            .as_nanos()
    ));
    create_dir_all(&dir)?;
    let path = dir.join("s.jsonl");
    let mut log = SessionLog::open(&path)?;
    log.append_user("inspect both")?;
    for id in ["a", "b"] {
        log.append_event(&Event::ToolCall {
            id: id.to_string(),
            name: "read_file".to_string(),
            arguments: format!(r#"{{"path":"{id}"}}"#),
        })?;
    }
    log.append_event(&Event::ToolResult {
        id: "a".to_string(),
        success: true,
        mutated: false,
        task: None,
        output: "first".to_string(),
    })?;
    assert_eq!(replay(&path)?.history, vec![Message::user("inspect both")]);

    log.append_event(&Event::ToolResult {
        id: "b".to_string(),
        success: true,
        mutated: false,
        task: None,
        output: "second".to_string(),
    })?;
    let history = replay(&path)?.history;
    assert_eq!(history.len(), 4);
    assert_eq!(history[1].tool_calls.len(), 2);
    assert_eq!(history[2].content, "first");
    assert_eq!(history[3].content, "second");
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn resume_recovers_only_an_unterminated_final_fragment() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "mage-session-tail-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| Error::Config(error.to_string()))?
            .as_nanos()
    ));
    create_dir_all(&dir)?;
    let path = dir.join("recover.jsonl");
    let mut log = SessionLog::open(&path)?;
    log.append_user("kept")?;
    drop(log);
    OpenOptions::new()
        .append(true)
        .open(&path)?
        .write_all(br#"{"seq":"#)?;

    let (mut resumed, replayed) = SessionLog::resume(&path)?;
    assert_eq!(replayed.history, vec![Message::user("kept")]);
    resumed.append_user("next")?;
    drop(resumed);
    assert_eq!(replay(&path)?.history.len(), 2);

    let bad = dir.join("bad.jsonl");
    let mut log = SessionLog::open(&bad)?;
    log.append_user("kept")?;
    drop(log);
    OpenOptions::new()
        .append(true)
        .open(&bad)?
        .write_all(b"{not json}\n")?;
    assert!(SessionLog::resume(&bad).is_err());
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn replay_restores_user_images_and_reads_legacy_users() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "mage-image-session-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| Error::Config(error.to_string()))?
            .as_nanos()
    ));
    create_dir_all(&dir)?;
    let path = dir.join("s.jsonl");
    let mut log = SessionLog::open(&path)?;
    log.append_input(&UserInput {
        text: "describe".to_string(),
        images: vec![Image::encode("image/png", b"\x89PNG\r\n\x1a\n")],
    })?;
    drop(log);
    let mut file = OpenOptions::new().append(true).open(&path)?;
    file.write_all(b"{\"kind\":\"user\",\"text\":\"legacy\"}\n")?;
    drop(file);

    let replayed = replay(&path)?;
    assert_eq!(replayed.history[0].images.len(), 1);
    assert_eq!(replayed.history[0].images[0].media_type, "image/png");
    assert_eq!(replayed.history[0].images[0].bytes, b"\x89PNG\r\n\x1a\n");
    assert!(replayed.history[1].images.is_empty());
    assert_eq!(replayed.history[1].content, "legacy");

    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn fork_copies_records_and_marks_the_parent() -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "mage-fork-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| Error::Config(error.to_string()))?
            .as_nanos()
    ));
    create_dir_all(&dir)?;
    let source = dir.join("s.jsonl");
    let mut log = SessionLog::open(&source)?;
    log.write(&Record::Header {
        id: "s".to_string(),
        cwd: dir.display().to_string(),
        model: "test-model".to_string(),
        prompt: "test".to_string(),
        parent: None,
    })?;
    log.append_user("original work")?;
    log.append_event(&Event::Assistant {
        text: "progress".to_string(),
    })?;
    drop(log);
    let (forked, replay) = SessionLog::fork_to(&source, &dir)?;
    assert_ne!(forked.id(), "s");
    let text = std::fs::read_to_string(forked.path())?;
    assert!(text.contains("\"parent\":\"s\""), "parent pointer recorded");
    assert!(
        replay
            .history
            .iter()
            .any(|message| message.content == "original work"),
        "history carried into the fork"
    );
    // the fork is independent: new records land only in the fork
    let mut forked = forked;
    forked.append_user("branch work")?;
    assert!(!std::fs::read_to_string(&source)?.contains("branch work"));
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
