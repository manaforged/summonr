use super::READ_PAGE_BYTES;
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
    let args = serde_json::json!({"path": "a.txt", "tags": true});
    let text = tools.read_file(&args, READ_PAGE_BYTES).unwrap().text;
    let shown = text.lines().find(|line| line.starts_with("2:")).unwrap();
    assert!(shown.contains("[line clipped at"), "{shown}");
    let tag = shown.split_once('|').unwrap().0;
    assert_eq!(tag, format!("2:{}", line_hash(&long)));
    assert_eq!(resolve_span(&content, tag, None).unwrap(), (1, 1));
}

#[tokio::test]
async fn multi_file_reads_keep_each_file_and_each_continuation() {
    let call = |args: &serde_json::Value| crate::provider::ToolCall {
        id: "read".to_string(),
        kind: crate::provider::ToolKind::Function,
        function: crate::provider::ToolFunction {
            name: "read".to_string(),
            arguments: args.to_string(),
            checked: Default::default(),
        },
        continuation: None,
    };
    for tags in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let names = ["alpha.txt", "beta.txt", "gamma.txt"];
        for (index, name) in names.iter().enumerate() {
            let content = (0..1000)
                .map(|line| format!("file{index} row{line} {}\n", "x".repeat(80)))
                .collect::<String>();
            std::fs::write(dir.path().join(name), content).unwrap();
        }
        let tools = crate::tools::ToolRegistry::with_policy_and_search(
            dir.path(),
            crate::policy::Policy::builtin(),
            None,
        )
        .unwrap();
        let output = tools
            .execute_outcome(&call(&serde_json::json!({"paths": names, "tags": tags})))
            .await
            .unwrap()
            .text;
        assert!(output.len() <= READ_PAGE_BYTES, "{}", output.len());
        assert!(!output.contains("not shown"), "{output}");
        for (index, name) in names.iter().enumerate() {
            let start = output.find(&format!("{name}  ")).unwrap();
            let section = &output[start..];
            assert!(section.contains(&format!("file{index} row0")), "{section}");
            let notice = section
                .lines()
                .find(|line| line.contains("continue with offset"))
                .unwrap();
            let offset: usize = notice
                .split("offset ")
                .nth(1)
                .unwrap()
                .trim_end_matches(']')
                .parse()
                .unwrap();
            let resumed = tools
                .execute_outcome(&call(
                    &serde_json::json!({"path": name, "offset": offset, "limit": 1}),
                ))
                .await
                .unwrap()
                .text;
            assert!(
                resumed.contains(&format!("file{index} row{}", offset - 1)),
                "{resumed}"
            );
        }
    }
}
