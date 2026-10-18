use std::path::{Path, PathBuf};

use serde_json::json;

use crate::support::{Model, Session, Step};

const KEPT: [&str; 3] = ["config.toml", "credentials.toml", "credentials.lock"];
const TORN: [&[u8]; 2] = [b"", b"{\"torn\":"];

fn state_files(root: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "objects" && name != "refs" && name != "logs" {
                state_files(&path, found);
            }
        } else if !KEPT.contains(&name.as_str())
            && !name.ends_with(".jsonl")
            && !name.ends_with(".log")
        {
            found.push(path);
        }
    }
}

pub(crate) fn a_session_resumes_with_any_state_file_torn() {
    let session = Session::new();
    let first = Model::start(vec![
        Step::Call("run_command", json!({"command": "echo one > note.txt"})),
        Step::Text("done"),
    ]);
    assert_eq!(session.run(&first, "", &[]).code, Some(0));
    let (listed, text) = session.cli(&["sessions"]);
    assert!(listed, "{text}");
    let mut files = Vec::new();
    state_files(session.home.path(), &mut files);
    state_files(&session.work.path().join(".git/summonr"), &mut files);
    assert!(files.len() >= 3, "too few state files to tear: {files:?}");
    for file in &files {
        let original = std::fs::read(file).unwrap();
        for torn in TORN {
            std::fs::write(file, torn).unwrap();
            let again = Model::start(vec![Step::Text("again")]);
            let outcome = session.run(&again, "", &["-c"]);
            assert_eq!(
                outcome.code,
                Some(0),
                "resuming with {} torn to {:?} failed: {}",
                file.display(),
                String::from_utf8_lossy(torn),
                outcome.stdout
            );
            std::fs::write(file, &original).unwrap();
        }
    }
}

pub(crate) fn a_torn_work_file_is_reported_on_resume() {
    let session = Session::new();
    let first = Model::start(vec![Step::Text("done")]);
    assert_eq!(session.run(&first, "", &[]).code, Some(0));
    let mut files = Vec::new();
    state_files(session.home.path(), &mut files);
    let work = files
        .iter()
        .find(|path| path.file_name().is_some_and(|name| name == "work.json"))
        .expect("the first run saved its work");
    std::fs::write(work, b"{\"torn\":").unwrap();
    let again = Model::start(vec![Step::Text("again")]);
    let outcome = session.run(&again, "", &["-c", "--json"]);
    assert_eq!(outcome.code, Some(0), "{}", outcome.stdout);
    assert!(
        outcome
            .stdout
            .lines()
            .any(|line| line.contains("\"type\":\"note\"") && line.contains("work.json")),
        "{}",
        outcome.stdout
    );
}
