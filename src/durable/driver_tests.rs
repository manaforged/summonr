use super::*;
use crate::provider::{ToolFunction, ToolKind};

#[test]
fn a_compacted_tail_replays_as_the_model_saw_it_after_an_interrupted_reply() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    let exposure = || Exposure {
        max_input_tokens: 0,
        max_output_tokens: 0,
    };
    let call = ToolCall {
        id: "c1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: "{}".to_string(),
        },
        continuation: None,
    };
    driver.prepare_turn_work("task", &[]).unwrap();
    driver.start_input(&UserInput::from("task")).unwrap();
    let attempt = driver.start_provider("model", exposure()).unwrap();
    let planned = driver
        .finish_provider_with_tools(
            attempt,
            "",
            "",
            Usage::default(),
            &[call],
            UsageMeta::default(),
        )
        .unwrap();
    driver
        .finish_tool(planned[0], "file body", true, false, 1, None)
        .unwrap();
    let seen = driver.history()[1..].to_vec();
    let attempt = driver.start_provider("model", exposure()).unwrap();
    driver.partial.text = "partial reply".to_string();
    driver
        .fail_provider(attempt, "stream cut", 0, false)
        .unwrap();
    driver.compact("summary", &seen).unwrap();
    assert_eq!(&driver.history()[2..], seen.as_slice());
}

#[test]
fn todo_list_does_not_outlive_clear() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    driver.prepare_turn_work("task", &[]).unwrap();
    driver
        .apply_todos(vec![crate::tools::Todo {
            text: "old task".to_string(),
            status: crate::tools::TodoStatus::Active,
        }])
        .unwrap();
    driver.clear().unwrap();
    driver.prepare_turn_work("next", &[]).unwrap();
    assert!(driver.work.as_ref().unwrap().todos.is_empty());
}

#[test]
fn steering_a_typed_steer_exports_whole_as_the_users_message() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    driver.prepare_turn_work("task", &[]).unwrap();
    driver.start_input(&UserInput::from("task")).unwrap();
    let typed = "Use staging.\nDo not run the migration.";
    driver
        .steer(&UserInput {
            text: typed.to_string(),
            images: Vec::new(),
            source: Some(crate::provider::Source {
                text: typed.to_string(),
                images: 0,
            }),
        })
        .unwrap();
    let exported = crate::session::export(driver.path()).unwrap();
    assert!(
        exported.contains(&format!("## You\n\n{typed}")),
        "{exported}"
    );
}
