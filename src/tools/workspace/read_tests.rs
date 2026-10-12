use crate::hashline::{line_hash, resolve_span};

#[test]
fn a_batch_read_with_every_path_failing_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "one\n").unwrap();
    let tools = crate::tools::ToolRegistry::with_policy_and_search(
        dir.path(),
        crate::policy::Policy::builtin(),
        None,
    )
    .unwrap();
    let gone = |name: &str| serde_json::Value::String(name.to_string());
    let both = tools.read_files(
        &serde_json::json!({}),
        &[gone("gone-a.txt"), gone("gone-b.txt")],
    );
    let Err(message) = both else {
        panic!("both paths are missing");
    };
    let message = message.to_string();
    assert!(message.contains("gone-a.txt"), "{message}");
    let mixed = tools
        .read_files(
            &serde_json::json!({}),
            &[gone("gone-a.txt"), gone("notes.txt")],
        )
        .unwrap();
    assert!(mixed.success);
    assert!(mixed.text.contains("gone-a.txt"));
    assert!(mixed.text.contains("notes.txt"));
}

#[test]
fn a_tag_on_a_clipped_long_line_resolves_against_the_full_line() {
    let dir = tempfile::tempdir().unwrap();
    let long = "x".repeat(10 * 1024);
    let content = format!("short\n{long}\nend\n");
    std::fs::write(dir.path().join("a.txt"), &content).unwrap();
    let tools = crate::tools::ToolRegistry::with_policy_and_search(
        dir.path(),
        crate::policy::Policy::builtin(),
        None,
    )
    .unwrap();
    let args = serde_json::json!({"path": "a.txt", "tags": true}).to_string();
    let text = tools.read_file(&args).unwrap().text;
    let shown = text.lines().find(|line| line.starts_with("2:")).unwrap();
    assert!(shown.contains("[line clipped at"), "{shown}");
    let tag = shown.split_once('|').unwrap().0;
    assert_eq!(tag, format!("2:{}", line_hash(&long)));
    assert_eq!(resolve_span(&content, tag, None).unwrap(), (1, 1));
}
