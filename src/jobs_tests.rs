use super::*;

fn finished(workspace: &Path, started: u64) -> PathBuf {
    let (id, record_path, _log, exit) = prepare(workspace).unwrap();
    let mut record = stamp(&id, 999_999, "true", workspace, true, None).unwrap();
    record.started = started;
    write(&record_path, &record).unwrap();
    write_exit(&exit, 0).unwrap();
    record_path
}

#[test]
fn a_finished_job_past_retention_is_pruned_and_a_recent_one_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let now = crate::mail::now();
    let old = finished(dir.path(), now - crate::limits::JOB_RETAIN.as_secs() - 60);
    let recent = finished(dir.path(), now);
    drop(since(dir.path(), 0, None));
    assert!(!old.exists());
    assert!(recent.exists());
}
