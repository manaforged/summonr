use super::*;

fn finished(workspace: &Path, started: u64) -> PathBuf {
    let (id, record_path, _log, exit) = prepare(workspace).unwrap();
    let mut record = stamp(&id, 999_999, "true", workspace, true, None).unwrap();
    record.started = started;
    write(&record_path, &record).unwrap();
    store::write(&exit, "0\n", store::Durability::Renamed).expect("write job exit fixture");
    record_path
}

#[test]
fn a_finished_job_past_retention_is_pruned_and_a_recent_one_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let now = crate::mail::now();
    let old = finished(
        dir.path(),
        now - crate::limits::limits().job_retain().as_secs() - 60,
    );
    let recent = finished(dir.path(), now);
    drop(since(dir.path(), 0, None));
    assert!(!old.exists());
    assert!(recent.exists());
}

#[test]
fn an_exit_marker_without_a_code_reads_as_a_dead_job() {
    let dir = tempfile::tempdir().unwrap();
    let (id, record_path, _log, exit) = prepare(dir.path()).unwrap();
    let record = stamp(&id, 999_999, "true", dir.path(), true, None).unwrap();
    write(&record_path, &record).unwrap();
    std::fs::write(&exit, "?\n").unwrap();
    let status = status(dir.path(), &record).unwrap();
    assert_eq!(status.state, JobState::Dead);
    assert_eq!(status.state.to_string(), "dead");
    assert!(status.text.contains("state: dead"), "{}", status.text);
}
