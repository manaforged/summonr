use std::path::{Path, PathBuf};

use crate::support::{self, Model, Session, Step};
use serde_json::json;

const WORK: &str = "/work/demo";
const HOME: &str = "/home/demo";
const SKIPPED: &[&str] = &[
    "config.toml",
    "credentials.toml",
    "host-id",
    "logs",
    "cache",
    "models",
    "tmp",
];

fn fixture_home() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat/v0.1.0/home")
}

fn forms(path: &Path) -> Vec<String> {
    let mut forms = vec![path.display().to_string()];
    if let Ok(real) = path.canonicalize() {
        forms.push(real.display().to_string());
    }
    forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
    forms.dedup();
    forms
}

fn copy(from: &Path, to: &Path, swap: &[(Vec<String>, String)], skip: &[&str]) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        if skip.contains(&name.as_str()) || name.ends_with(".lock") || name == "pruned" {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target, swap, &[]);
            continue;
        }
        let bytes = std::fs::read(entry.path()).unwrap();
        let Ok(mut text) = String::from_utf8(bytes.clone()) else {
            std::fs::write(&target, bytes).unwrap();
            continue;
        };
        for (olds, new) in swap {
            for old in olds {
                text = text.replace(old.as_str(), new);
            }
        }
        std::fs::write(&target, text).unwrap();
    }
}

fn goal(session: &Session) {
    std::fs::create_dir_all(session.work.path().join(".summonr")).unwrap();
    std::fs::write(
        session.work.path().join(".summonr/check.toml"),
        "check = 'exit 0'\n",
    )
    .unwrap();
    std::fs::write(session.work.path().join("notes.txt"), "draft\n").unwrap();
}

pub(crate) fn write_fixture(out: std::ffi::OsString) {
    let session = Session::new();
    goal(&session);
    let model = Model::start(vec![
        Step::Call("write", json!({"path": "notes.txt", "content": "done\n"})),
        Step::Text("notes.txt now says done"),
        Step::Text(r#"{"verdict": "done", "reason": "the check passed"}"#),
    ]);
    let result = session.run(&model, "", &["--goal", "finish the notes", "--json"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    let (ok, text) = session.cli(&[
        "cron",
        "add",
        "nightly",
        "--cron",
        "0 3 * * *",
        "--goal",
        "make the tests pass",
    ]);
    assert!(ok, "{text}");
    let swap = [
        (forms(session.work.path()), WORK.to_string()),
        (forms(session.home.path()), HOME.to_string()),
    ];
    let out = PathBuf::from(out);
    drop(std::fs::remove_dir_all(&out));
    copy(session.home.path(), &out, &swap, SKIPPED);
}

fn restored() -> Session {
    let session = Session::new();
    goal(&session);
    std::fs::write(session.work.path().join("notes.txt"), "done\n").unwrap();
    let real = |path: &Path| path.canonicalize().unwrap().display().to_string();
    let swap = [
        (vec![WORK.to_string()], real(session.work.path())),
        (vec![HOME.to_string()], real(session.home.path())),
    ];
    copy(&fixture_home(), session.home.path(), &swap, &[]);
    session
}

pub(crate) fn sessions_saved_by_0_1_0_list_export_and_profile() {
    let session = restored();
    let (ok, list) = session.cli(&["sessions"]);
    let workspace = session
        .work
        .path()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    assert!(
        ok && list.contains(&workspace) && list.contains("01a1559a2020"),
        "{list}"
    );
    let (ok, markdown) = session.cli(&["export"]);
    assert!(
        ok && markdown.contains("notes.txt now says done"),
        "{markdown}"
    );
    let (ok, profile) = session.cli(&["profile", "--all"]);
    assert!(
        ok && profile.contains("1 sessions (0 skipped unreadable)"),
        "{profile}"
    );
}

pub(crate) fn a_cron_job_saved_by_0_1_0_still_lists() {
    let session = restored();
    let (ok, list) = session.cli(&["cron", "list", "--json"]);
    assert!(ok, "{list}");
    let job: serde_json::Value = serde_json::from_str(list.trim()).unwrap();
    assert_eq!(job["name"], "nightly");
    assert_eq!(job["schedule"], "0 3 * * *");
}

pub(crate) fn a_session_saved_by_0_1_0_resumes_with_its_history() {
    let session = restored();
    let model = Model::start(vec![Step::Text("resumed")]);
    let result = session.run(&model, "", &["-c"]);
    let stderr = support::read(&session.file("stderr"));
    assert_eq!(result.code, Some(0), "{}\n{stderr}", result.stdout);
    let sent = model
        .requests()
        .iter()
        .map(ToString::to_string)
        .collect::<String>();
    assert!(
        sent.contains("finish the notes"),
        "the resumed turn lost its history: {sent}"
    );
}
