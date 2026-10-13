use crate::policy::Policy;
use crate::provider::{ToolCall, ToolFunction, ToolKind};
use crate::tools::ToolRegistry;

fn names(definitions: &[crate::provider::ToolDefinition]) -> Vec<String> {
    definitions
        .iter()
        .map(|tool| tool.function.name.clone())
        .collect()
}

#[test]
fn gpt_models_edit_through_apply_patch_and_others_keep_edit_and_write() {
    let dir = tempfile::tempdir().unwrap();
    let tools = ToolRegistry::with_policy_and_search(dir.path(), Policy::builtin(), None).unwrap();
    for model in ["gpt-6.1-sol", "gpt-6-astra", "gpt-5.5"] {
        assert!(crate::provider::traits(model).patch, "{model}");
    }
    for model in [
        "claude-opus-5-5",
        "gemini-3.8-flash",
        "glm-5.3",
        "gpt-4o",
        "gpt-oss-120b",
    ] {
        assert!(!crate::provider::traits(model).patch, "{model}");
    }
    let patched = names(&super::shaped(tools.definitions(), true));
    assert!(patched.contains(&"apply_patch".to_string()), "{patched:?}");
    assert!(!patched.contains(&"edit".to_string()), "{patched:?}");
    assert!(!patched.contains(&"write".to_string()), "{patched:?}");
    let plain = names(&super::shaped(tools.definitions(), false));
    assert!(plain.contains(&"edit".to_string()) && plain.contains(&"write".to_string()));
    assert!(!plain.contains(&"apply_patch".to_string()));
}

#[test]
fn an_apply_patch_call_edits_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    let tools = ToolRegistry::with_policy_and_search(dir.path(), Policy::builtin(), None).unwrap();
    let patch = "*** Begin Patch\n*** Update File: a.txt\n@@\n one\n-two\n+three\n*** End Patch\n";
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "apply_patch".to_string(),
            arguments: serde_json::json!({ "patch": patch }).to_string(),
        },
        continuation: None,
    };
    let outcome = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call))
        .unwrap();
    assert!(outcome.success, "{}", outcome.text);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\nthree\n"
    );
}
