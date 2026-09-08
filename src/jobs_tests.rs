use super::{Record, live, since, write};

fn record(dir: &std::path::Path, id: &str, started: u64, session: Option<&str>) -> Record {
    let record = Record {
        id: id.to_string(),
        pid: std::process::id(),
        command: format!("job {id}"),
        workspace: dir.display().to_string(),
        started,
        background: true,
        sandbox: false,
        session: session.map(str::to_string),
    };
    let root = super::dir(dir);
    std::fs::create_dir_all(&root).expect("jobs dir");
    write(&root.join(format!("{id}.json")), &record).expect("write");
    record
}

#[test]
fn jobs_are_filtered_by_start_time_and_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    record(dir.path(), "a", now - 30, Some("s1"));
    record(dir.path(), "b", now - 20, Some("s2"));
    record(dir.path(), "c", now - 10, None);
    let ids = |records: Vec<Record>| records.into_iter().map(|r| r.id).collect::<Vec<_>>();
    assert_eq!(ids(since(dir.path(), 0, None)), ["a", "b", "c"]);
    assert_eq!(ids(since(dir.path(), now - 25, Some("s2"))), ["b", "c"]);
    assert_eq!(ids(since(dir.path(), 0, Some("s1"))), ["a", "c"]);
    let alive = live(dir.path());
    assert!(
        alive.iter().all(|line| line.contains("job kill")),
        "the remedy is named: {alive:?}"
    );
    record(dir.path(), "old", now - 3600, None);
    assert!(
        !live(dir.path()).iter().any(|line| line.starts_with("old ")),
        "a record older than its pid's process is not alive"
    );
}

#[test]
fn a_killed_job_is_neutral_and_a_reused_pid_is_not_alive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = super::dir(dir.path());
    std::fs::create_dir_all(&root).expect("jobs dir");
    let killed = record(dir.path(), "k", 10, None);
    super::write_killed(&super::exit_path(dir.path(), &killed.id)).expect("killed");
    let status = super::status(dir.path(), "k").expect("status");
    assert!(status.killed && !status.running && status.exit_code.is_none());
    assert!(status.text.contains("state: killed"), "{}", status.text);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    assert!(
        super::alive_since(std::process::id(), now),
        "this process started now-ish"
    );
    assert!(
        !super::alive_since(std::process::id(), now - 3600),
        "a record an hour older than the process is a reused pid"
    );
}

#[test]
fn ps_elapsed_parses_both_shapes() {
    assert_eq!(super::elapsed_secs("05"), Some(5));
    assert_eq!(super::elapsed_secs("01:05"), Some(65));
    assert_eq!(super::elapsed_secs("02:01:05"), Some(7265));
    assert_eq!(super::elapsed_secs("1-02:01:05"), Some(93_665));
    assert_eq!(super::elapsed_secs(""), None);
}
