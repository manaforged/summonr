use super::*;

fn home(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mage-bus-{}-{}-{tag}", std::process::id(), now()))
}

fn bus_at(dir: &Path, id: &str) -> Bus {
    Bus {
        id: id.to_string(),
        dir: dir.to_path_buf(),
        cursor: 0,
        last_beat: 0,
        status: String::new(),
    }
}

fn setup(tag: &str) -> (PathBuf, PathBuf) {
    let dir = home(tag);
    std::fs::create_dir_all(dir.join("agents"))
        .unwrap_or_else(|error| panic!("create agents directory: {error}"));
    std::fs::create_dir_all(dir.join("inbox"))
        .unwrap_or_else(|error| panic!("create inbox directory: {error}"));
    std::fs::create_dir_all(dir.join("board"))
        .unwrap_or_else(|error| panic!("create board directory: {error}"));
    let cwd = dir.join("ws");
    std::fs::create_dir_all(&cwd)
        .unwrap_or_else(|error| panic!("create workspace directory: {error}"));
    (dir, cwd)
}

#[test]
fn heartbeat_makes_and_expires_presence() {
    let (dir, cwd) = setup("presence");
    let mut a = bus_at(&dir, "a");
    a.heartbeat("glm", &cwd, "idle");
    let roster = a.agents();
    assert_eq!(roster.len(), 1);
    assert_eq!(roster[0].id, "a");
    assert_eq!(roster[0].status, "idle");
    // a stale record drops off the roster
    let stale = Agent {
        id: "ghost".to_string(),
        model: "glm".to_string(),
        cwd: cwd.display().to_string(),
        pid: 1,
        status: "gone".to_string(),
        last_seen: now().saturating_sub(STALE_SECS + 10),
    };
    std::fs::write(
        dir.join("agents/ghost.json"),
        serde_json::to_string(&stale).unwrap_or_default(),
    )
    .unwrap_or_else(|error| panic!("write stale agent: {error}"));
    assert_eq!(a.agents().len(), 1, "stale record is not live");
    a.leave();
    assert!(a.agents().is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn send_and_poll_with_cursor() {
    let (dir, cwd) = setup("mail");
    let mut a = bus_at(&dir, "a");
    let mut b = bus_at(&dir, "b");
    a.heartbeat("glm", &cwd, "idle");
    b.heartbeat("glm", &cwd, "idle");
    assert_eq!(a.send("b", "hello b").unwrap_or(0), 1);
    assert_eq!(a.send("all", "hi everyone").unwrap_or(0), 1);
    let mail = b.poll();
    assert_eq!(mail.len(), 2);
    assert_eq!(mail[0].from, "a");
    assert_eq!(mail[0].text, "hello b");
    assert!(b.poll().is_empty(), "cursor consumed the mail");
    // broadcast skips the sender
    assert!(a.poll().is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn board_last_write_wins() {
    let (dir, cwd) = setup("board");
    let a = bus_at(&dir, "a");
    let b = bus_at(&dir, "b");
    a.post(&cwd, "claim:src/tui.rs", "a")
        .unwrap_or_else(|error| panic!("post first claim: {error}"));
    b.post(&cwd, "decision", "use rmcp")
        .unwrap_or_else(|error| panic!("post decision: {error}"));
    b.post(&cwd, "claim:src/tui.rs", "b")
        .unwrap_or_else(|error| panic!("post second claim: {error}"));
    let board = a.board(&cwd);
    assert_eq!(board.len(), 2);
    let claim = board.iter().find(|(k, _, _)| k == "claim:src/tui.rs");
    assert_eq!(
        claim.map(|(_, v, from)| (v.as_str(), from.as_str())),
        Some(("b", "b")),
        "last write wins"
    );
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn hostile_ids_cannot_escape_the_bus_dir() {
    let (dir, cwd) = setup("traversal");
    let mut a = bus_at(&dir, "a");
    let mut b = bus_at(&dir, "b");
    a.heartbeat("glm", &cwd, "idle");
    b.heartbeat("glm", &cwd, "idle");
    // a traversal attempt writes inside inbox/, never outside
    assert_eq!(b.send("../../etc/evil", "pwned").unwrap_or(0), 1);
    assert!(!dir.join("etc").exists());
    assert!(dir.join("inbox/..-..-etc-evil.jsonl").exists());
    // and the sanitized target is readable as a normal mailbox
    let mut evil = bus_at(&dir, "../../etc/evil");
    assert_eq!(evil.poll().len(), 1);
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn mail_truncates_and_oversized_inboxes_rotate() {
    let (dir, cwd) = setup("caps");
    let mut a = bus_at(&dir, "a");
    let mut b = bus_at(&dir, "b");
    a.heartbeat("glm", &cwd, "idle");
    b.heartbeat("glm", &cwd, "idle");
    let long = "x".repeat(MAIL_CHARS * 3);
    a.send("b", &long)
        .unwrap_or_else(|error| panic!("send long message: {error}"));
    let mail = b.poll();
    assert_eq!(mail.len(), 1);
    assert_eq!(mail[0].text.chars().count(), MAIL_CHARS);
    // an oversized mailbox rotates and stays parseable
    let path = dir.join("inbox/b.jsonl");
    let fat = format!("{}\n", "junk\n".repeat(120_000));
    std::fs::write(&path, &fat).unwrap_or_else(|error| panic!("write oversized inbox: {error}"));
    assert!(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES as u64);
    drop(b.poll());
    let shrunk = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    assert!(shrunk < MAX_FILE_BYTES as u64, "rotated below the cap");
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn ancient_records_are_swept() {
    let (dir, cwd) = setup("sweep");
    let mut a = bus_at(&dir, "a");
    a.heartbeat("glm", &cwd, "idle");
    let ancient = Agent {
        id: "old".to_string(),
        model: "glm".to_string(),
        cwd: cwd.display().to_string(),
        pid: 1,
        status: "gone".to_string(),
        last_seen: now().saturating_sub(SWEEP_SECS + 10),
    };
    let path = dir.join("agents/old.json");
    std::fs::write(&path, serde_json::to_string(&ancient).unwrap_or_default())
        .unwrap_or_else(|error| panic!("write ancient agent: {error}"));
    drop(a.agents());
    assert!(!path.exists(), "day-old record swept");
    assert_eq!(a.agents().len(), 1, "fresh record stays");
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn bus_is_off_without_explicit_configuration() {
    assert!(!bus_enabled(None));
    assert!(bus_enabled(Some(std::ffi::OsStr::new("on"))));
    assert!(bus_enabled(Some(std::ffi::OsStr::new("true"))));
    assert!(!bus_enabled(Some(std::ffi::OsStr::new("off"))));
}

#[test]
fn bus_tool_schema_is_stable_across_modes() -> Result<()> {
    let (dir, cwd) = setup("schemas");
    let registry = crate::tools::ToolRegistry::new(&cwd)?;
    let base = registry.definitions();
    let definitions = definitions();
    assert!(base.iter().all(|tool| !is_bus_tool(&tool.function.name)));
    assert_eq!(definitions.len(), 4);
    assert!(definitions.iter().any(|tool| tool.function.name == "send"));
    drop(std::fs::remove_dir_all(&dir));
    Ok(())
}
