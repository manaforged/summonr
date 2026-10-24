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

#[test]
fn a_prepared_job_folder_is_ignored_by_git() {
    let dir = tempfile::tempdir().unwrap();
    prepare(dir.path()).unwrap();
    let ignore = std::fs::read_to_string(dir.path().join(".summonr/jobs/.gitignore")).unwrap();
    assert_eq!(ignore, "*\n");
}

#[test]
fn a_stopped_job_reads_as_killed() {
    let dir = tempfile::tempdir().unwrap();
    let (id, record_path, _log, _exit) = prepare(dir.path()).unwrap();
    let record = stamp(&id, 999_999, "sleep 9", dir.path(), true, None).unwrap();
    write(&record_path, &record).unwrap();
    write_killed(&exit_path(dir.path(), &id), &[]).unwrap();
    let status = status(dir.path(), &record).unwrap();
    assert_eq!(status.state, JobState::Killed);
}

#[cfg(unix)]
fn call(tools: &crate::tools::ToolRegistry, arguments: serde_json::Value) -> String {
    let call = crate::provider::ToolCall {
        id: "t1".to_string(),
        kind: crate::provider::ToolKind::Function,
        function: crate::provider::ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: arguments.to_string(),
        },
        continuation: None,
    };
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call))
        .unwrap()
        .text
}

#[cfg(unix)]
fn started(dir: &Path, command: &str) -> (crate::tools::ToolRegistry, String) {
    let tools = crate::tools::ToolRegistry::with_policy_and_search(
        dir,
        crate::policy::Policy::builtin(),
        None,
    )
    .unwrap();
    tools.set_session("s1");
    let text = call(
        &tools,
        serde_json::json!({"command": command, "background": true}),
    );
    let id = text
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .unwrap_or_else(|| panic!("{text}"))
        .to_string();
    (tools, id)
}

#[cfg(unix)]
#[test]
fn a_running_job_status_names_its_child_commands() {
    let dir = tempfile::tempdir().unwrap();
    let (tools, id) = started(dir.path(), "sleep 41 & sleep 42; wait");
    let status = call(&tools, serde_json::json!({"job": id}));
    drop(tools.stop_jobs());
    assert!(status.contains("children:"), "{status}");
    assert!(status.contains("sleep 41"), "{status}");
    assert!(status.contains("sleep 42"), "{status}");
}

#[cfg(unix)]
#[test]
fn a_cancel_names_the_child_commands_it_killed() {
    let dir = tempfile::tempdir().unwrap();
    let (tools, id) = started(dir.path(), "sleep 43; true");
    let running = call(&tools, serde_json::json!({"job": id}));
    assert!(running.contains("sleep 43"), "{running}");
    let notes = tools.stop_jobs().unwrap();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains(&format!("killed job {id}")),
        "{}",
        notes[0]
    );
    assert!(notes[0].contains("child commands at kill:"), "{}", notes[0]);
    assert!(notes[0].contains("sleep 43"), "{}", notes[0]);
    let status = call(&tools, serde_json::json!({"job": id}));
    assert!(status.contains("children at kill:"), "{status}");
    assert!(status.contains("sleep 43"), "{status}");
}

#[cfg(unix)]
#[test]
fn a_child_command_with_newlines_lists_on_one_line() {
    let dir = tempfile::tempdir().unwrap();
    let (tools, id) = started(dir.path(), "sleep 44\nsleep 45");
    let status = call(&tools, serde_json::json!({"job": id}));
    drop(tools.stop_jobs());
    let block: Vec<&str> = status
        .lines()
        .skip_while(|line| *line != "children:")
        .skip(1)
        .take_while(|line| !line.starts_with("log:"))
        .collect();
    assert!(!block.is_empty(), "{status}");
    assert!(
        block.iter().all(|line| line
            .trim_start()
            .split(' ')
            .next()
            .unwrap()
            .parse::<u32>()
            .is_ok()),
        "{status}"
    );
}
