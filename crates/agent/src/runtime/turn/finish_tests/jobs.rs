use super::*;
use crate::tools::AbortCode;

fn job(run: &Run, command: &str, session: Option<String>) -> (String, PathBuf) {
    let workspace = run._dirs[0].path();
    let (id, path, _, exit) = crate::jobs::prepare(workspace).unwrap();
    crate::jobs::write(
        &path,
        &crate::jobs::Record {
            id: id.clone(),
            pid: std::process::id(),
            command: command.to_string(),
            workspace: workspace.display().to_string(),
            started: summonr_core::clock::now(),
            background: true,
            session,
            log: None,
            detached: false,
        },
    )
    .unwrap();
    (id, exit)
}

fn exit_on_second_request(exit: Arc<Mutex<PathBuf>>, replies: Vec<Reply>) -> Scripted {
    Scripted::new(replies).on_complete(move |round| {
        if round == 1 {
            std::fs::write(&*exit.lock().unwrap(), "0").unwrap();
        }
    })
}

#[tokio::test]
async fn a_job_that_exits_during_a_later_turn_is_reported_once() {
    let exit = Arc::new(Mutex::new(PathBuf::new()));
    let replies = vec![Reply::Text("one"), Reply::Text("two")];
    let mut run = open(exit_on_second_request(Arc::clone(&exit), replies));
    let session = run.loop_.session_id();
    let (id, path) = job(&run, "long build", Some(session));
    *exit.lock().unwrap() = path;
    run.loop_.turn("start the build").await.unwrap();
    assert!(run.loop_.finished_jobs().is_empty());
    run.loop_.turn("an unrelated question").await.unwrap();
    let notes = run.loop_.finished_jobs();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains(&id), "{notes:?}");
    assert!(run.loop_.finished_jobs().is_empty());
}

#[tokio::test]
async fn a_job_that_exited_between_turns_reaches_the_next_harness_note_once() {
    let mut run = session([Reply::Text("one"), Reply::Text("two"), Reply::Text("three")]);
    let session_id = run.loop_.session_id();
    let (_id, exit) = job(&run, "long build", Some(session_id));
    run.loop_.turn("start the build").await.unwrap();
    std::fs::write(exit, "0").unwrap();
    run.loop_.turn("next").await.unwrap();
    run.loop_.turn("after that").await.unwrap();
    let sent = run.sent.lock().unwrap();
    let says = |round: usize| {
        sent[round]
            .last()
            .unwrap()
            .content
            .contains("finished (exit 0)")
    };
    assert!(says(1), "{:?}", sent[1]);
    assert!(!says(2), "{:?}", sent[2]);
}

#[tokio::test]
async fn a_job_in_goal_mode_is_watched_when_the_turn_ends() {
    let mut run = session([Reply::Text("waiting")]);
    run.loop_.select_goal("ship it", &[]).unwrap();
    run.loop_.set_mode(crate::WorkKind::Goal);
    let session_id = run.loop_.session_id();
    let (id, exit) = job(&run, "long build", Some(session_id));
    run.loop_.turn("ship it").await.unwrap();
    std::fs::write(exit, "0").unwrap();
    let notes = run.loop_.finished_jobs();
    assert!(notes.iter().any(|note| note.contains(&id)), "{notes:?}");
}

#[tokio::test]
async fn a_job_this_session_does_not_own_is_ignored() {
    let mut run = session([Reply::Text("one")]);
    let (_none, none_exit) = job(&run, "no owner", None);
    let (_other, other_exit) = job(&run, "other owner", Some("another-session".to_string()));
    run.loop_.turn("start").await.unwrap();
    std::fs::write(none_exit, "0").unwrap();
    std::fs::write(other_exit, "0").unwrap();
    assert!(run.loop_.finished_jobs().is_empty());
}

#[tokio::test]
async fn a_job_the_model_already_polled_starts_no_turn() {
    let mut run = session([]);
    let session_id = run.loop_.session_id();
    let (id, exit) = job(&run, "long build", Some(session_id));
    run.loop_.watch_jobs();
    std::fs::write(exit, "0").unwrap();
    let polled = run
        .loop_
        .tools
        .execute_outcome(&ToolCall {
            id: "poll".to_string(),
            kind: ToolKind::Function,
            function: ToolFunction {
                checked: Default::default(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({ "job": id }).to_string(),
            },
            continuation: None,
        })
        .await
        .unwrap();
    assert!(polled.success, "{}", polled.text);
    assert!(run.loop_.finished_jobs().is_empty());
}

#[tokio::test]
async fn a_resume_reports_a_job_that_exited_while_the_session_was_closed() {
    let mut run = session([Reply::Text("one")]);
    let session_id = run.loop_.session_id();
    let (id, exit) = job(&run, "long build", Some(session_id));
    run.loop_.turn("start the build").await.unwrap();
    let path = run.loop_.session_path().to_path_buf();
    let workspace = run._dirs[0].path().to_path_buf();
    let Run { loop_, _dirs, .. } = run;
    drop(loop_);
    std::fs::write(exit, "0").unwrap();
    let mut resumed =
        Loop::resume_path(Scripted::new([]), "m", &workspace, "system", &path).unwrap();
    let notes = resumed.finished_jobs();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains(&id), "{notes:?}");
}

#[cfg(unix)]
fn detach(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({ "command": "sleep 30", "timeout_secs": 1 }).to_string(),
        },
        continuation: None,
    }
}

#[cfg(unix)]
fn started_job(text: &str) -> &str {
    text.lines()
        .find_map(|line| line.strip_prefix("started job "))
        .unwrap_or_else(|| panic!("no job id in {text}"))
}

#[cfg(unix)]
#[tokio::test]
async fn escape_stops_only_the_job_this_turn_detached() {
    let mut run = session([]);
    let workspace = run._dirs[0].path().to_path_buf();
    run.loop_
        .begin_turn(UserInput::from("first"), Trigger::User)
        .unwrap();
    let older = run.loop_.tools.execute_outcome(&detach("a")).await.unwrap();
    let older = crate::jobs::load(&workspace, started_job(&older.text)).unwrap();
    run.loop_.session.close().unwrap();
    run.loop_
        .begin_turn(UserInput::from("second"), Trigger::User)
        .unwrap();
    let newer = run.loop_.tools.execute_outcome(&detach("b")).await.unwrap();
    let newer = crate::jobs::load(&workspace, started_job(&newer.text)).unwrap();
    run.loop_
        .abort_with(
            &Abort {
                code: AbortCode::Cancelled,
                reason: "cancelled by user".to_string(),
            },
            |_| {},
        )
        .unwrap();
    let state = |record| crate::jobs::status(&workspace, record).unwrap().state;
    assert_eq!(state(&older), crate::jobs::JobState::Running);
    assert_eq!(state(&newer), crate::jobs::JobState::Killed);
    run.loop_.tools.stop_jobs().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_provider_failure_keeps_an_explicit_background_job() {
    let mut run = session([]);
    let workspace = run._dirs[0].path().to_path_buf();
    run.loop_
        .begin_turn(UserInput::from("start"), Trigger::User)
        .unwrap();
    let call = ToolCall {
        id: "bg".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({ "command": "sleep 30", "background": true }).to_string(),
        },
        continuation: None,
    };
    let started = run.loop_.tools.execute_outcome(&call).await.unwrap();
    let record = crate::jobs::load(&workspace, started_job(&started.text)).unwrap();
    run.loop_
        .abort_with(
            &Abort {
                code: AbortCode::Error,
                reason: "turn failed".to_string(),
            },
            |_| {},
        )
        .unwrap();
    assert_eq!(
        crate::jobs::status(&workspace, &record).unwrap().state,
        crate::jobs::JobState::Running
    );
    run.loop_.tools.stop_jobs().unwrap();
}
