use std::path::{Path, PathBuf};

use super::{latest_in, list_in, resolve_in};
use crate::durable::log;

const OLDER: &str = "0199a1b2c3d47e5f8a9b0c1d2e3f4a5b";
const MIDDLE: &str = "0199a1b2c3d57e5f8a9b0c1d2e3f4a5c";
const NEWER: &str = "01a0c4d5e6f77a8b9c0d1e2f3a4b5c6d";

fn journal(home: &Path, id: &str) -> PathBuf {
    let path = log::sessions_dir(home).join(id).join("session.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    path
}

fn header(id: &str, workspace: &str, depth: u8) -> String {
    let workspace = serde_json::Value::from(workspace);
    format!(
        r#"{{"kind":"session","id":"{id}","model":"glm","system":"","workspace":{workspace},"depth":{depth}}}"#
    ) + "\n"
}

fn prompt(text: &str) -> String {
    format!(r#"{{"kind":"user","text":"{text}","start":true}}"#) + "\n"
}

fn session(home: &Path, id: &str, workspace: &str, depth: u8) -> PathBuf {
    let path = journal(home, id);
    std::fs::write(&path, header(id, workspace, depth) + &prompt(id)).unwrap();
    path
}

#[test]
fn resume_finds_a_named_session_not_the_newest() {
    let home = tempfile::tempdir().unwrap();
    let older = session(home.path(), OLDER, "/work/alpha", 0);
    session(home.path(), NEWER, "/work/alpha", 0);
    super::set_name(&older, "leak-hunt").unwrap();
    assert_eq!(resolve_in(home.path(), "leak-hunt").unwrap(), older);
}

#[test]
fn resume_resolves_a_short_id_and_an_id_suffix() {
    let home = tempfile::tempdir().unwrap();
    let older = session(home.path(), OLDER, "/work/alpha", 0);
    let middle = session(home.path(), MIDDLE, "/work/alpha", 0);
    session(home.path(), NEWER, "/work/alpha", 0);
    assert_eq!(
        resolve_in(home.path(), super::short(MIDDLE)).unwrap(),
        middle
    );
    assert_eq!(resolve_in(home.path(), &OLDER[26..]).unwrap(), older);
    let shared = resolve_in(home.path(), &OLDER[..11]).unwrap_err();
    assert!(shared.to_string().contains("ambiguous"), "{shared}");
}

#[test]
fn continue_picks_the_newest_top_level_session_of_this_workspace() {
    let home = tempfile::tempdir().unwrap();
    let alpha = session(home.path(), OLDER, "/work/alpha", 0);
    session(home.path(), MIDDLE, "/work/alpha", 1);
    session(home.path(), NEWER, "/work/beta", 0);
    let found = latest_in(home.path(), Path::new("/work/alpha"), None).unwrap();
    assert_eq!(found.map(|info| info.path), Some(alpha));
}

#[test]
fn continue_from_a_linked_worktree_finds_the_primary_checkouts_session() {
    let (root, primary) = crate::worktree::tests::repo();
    let linked = root.path().join("linked");
    crate::worktree::tests::git(
        &primary,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            &linked.display().to_string(),
        ],
    );
    let home = tempfile::tempdir().unwrap();
    let primary = dunce::canonicalize(&primary).unwrap();
    let started = session(home.path(), OLDER, &primary.display().to_string(), 0);
    let found = latest_in(home.path(), &linked, None).unwrap();
    assert_eq!(found.map(|info| info.path), Some(started));
}

#[test]
fn a_session_list_reads_a_prompt_written_after_the_header() {
    let home = tempfile::tempdir().unwrap();
    let path = journal(home.path(), OLDER);
    let head = header(OLDER, "/work/alpha", 0);
    std::fs::write(&path, &head).unwrap();
    assert_eq!(list_in(home.path()).unwrap()[0].preview, "");
    std::fs::write(&path, head + &prompt("trace the leak")).unwrap();
    assert_eq!(list_in(home.path()).unwrap()[0].preview, "trace the leak");
}

#[test]
fn an_empty_first_prompt_is_not_the_preview() {
    let home = tempfile::tempdir().unwrap();
    let path = journal(home.path(), OLDER);
    let body = header(OLDER, "/work/alpha", 0) + &prompt("") + &prompt("trace the leak");
    std::fs::write(&path, body).unwrap();
    assert_eq!(list_in(home.path()).unwrap()[0].preview, "trace the leak");
}

#[test]
fn a_deleted_journal_leaves_the_list_and_continue() {
    let home = tempfile::tempdir().unwrap();
    session(home.path(), OLDER, "/work/alpha", 0);
    let gone = session(home.path(), NEWER, "/work/alpha", 0);
    assert_eq!(list_in(home.path()).unwrap().len(), 2);
    std::fs::remove_file(&gone).unwrap();
    let listed: Vec<PathBuf> = list_in(home.path())
        .unwrap()
        .into_iter()
        .map(|info| info.path)
        .collect();
    assert!(!listed.contains(&gone), "{listed:?}");
    let found = latest_in(home.path(), Path::new("/work/alpha"), None).unwrap();
    assert_ne!(found.map(|info| info.path), Some(gone));
}

#[test]
fn continue_ignores_a_linked_worktree_that_summonr_did_not_make() {
    let (root, primary) = crate::worktree::tests::repo();
    let foreign = root.path().join("feature");
    crate::worktree::tests::git(
        &primary,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            &foreign.display().to_string(),
        ],
    );
    let foreign = dunce::canonicalize(&foreign).unwrap();
    let home = tempfile::tempdir().unwrap();
    session(home.path(), OLDER, &foreign.display().to_string(), 0);
    let found = latest_in(home.path(), &primary, None).unwrap();
    assert_eq!(found.map(|info| info.path), None);
}

#[test]
fn an_open_session_of_unchanged_size_is_served_from_the_index() {
    let home = tempfile::tempdir().unwrap();
    let path = journal(home.path(), OLDER);
    std::fs::write(&path, header(OLDER, "/work/alpha", 0)).unwrap();
    assert_eq!(
        list_in(home.path()).unwrap()[0].cwd.as_deref(),
        Some("/work/alpha")
    );
    std::fs::write(&path, header(OLDER, "/work/omega", 0)).unwrap();
    assert_eq!(
        list_in(home.path()).unwrap()[0].cwd.as_deref(),
        Some("/work/alpha")
    );
}

#[test]
fn sessions_started_in_one_millisecond_are_named_when_their_short_id_is_ambiguous() {
    const FIRST: &str = "01a16468026a70c39b593c8d424a8179";
    const SECOND: &str = "01a16468026a7b40b5e58668d9b67cd8";
    let home = tempfile::tempdir().unwrap();
    session(home.path(), FIRST, "/work/alpha", 0);
    let second = session(home.path(), SECOND, "/work/alpha", 0);
    let error = resolve_in(home.path(), super::short(FIRST))
        .unwrap_err()
        .to_string();
    assert!(error.contains(FIRST) && error.contains(SECOND), "{error}");
    assert_eq!(resolve_in(home.path(), &SECOND[..16]).unwrap(), second);
    let removed = super::remove_in(home.path(), super::short(FIRST))
        .unwrap_err()
        .to_string();
    assert!(
        removed.contains(FIRST) && removed.contains(SECOND),
        "{removed}"
    );
}
