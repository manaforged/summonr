use std::path::PathBuf;

use super::*;
use crate::policy::Policy;

#[test]
fn empty_run_command_arguments_are_a_tool_error() {
    let error = parse_args("run_command", "").unwrap_err();
    let Error::Tool(message) = error else {
        panic!("{error}");
    };
    assert!(
        message.contains("run_command arguments are invalid"),
        "{message}"
    );
}

#[test]
fn an_unmounted_volume_is_not_created() {
    let dir = tempfile::tempdir().unwrap();
    let mut tools =
        ToolRegistry::with_policy_and_search(dir.path(), Policy::builtin(), None).unwrap();
    let path = PathBuf::from("/Volumes/SummonrNoSuchVolume/dev");
    let error = tools.allow_path(path.clone()).unwrap_err();
    assert!(matches!(error, Error::Config(_)), "{error}");
    assert!(!path.exists());
    assert!(!PathBuf::from("/Volumes/SummonrNoSuchVolume").exists());
}

#[test]
fn todo_accepts_the_todos_key_models_actually_send() {
    let items =
        todos_from_args(r#"{"todos":[{"title":"decode the image","status":"in_progress"}]}"#)
            .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].text, "decode the image");
    assert_eq!(items[0].status, TodoStatus::Active);
}
