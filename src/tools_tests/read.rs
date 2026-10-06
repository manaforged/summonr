use super::*;

#[test]
fn todo_is_advertised_only_where_the_ledger_is_read() {
    let (_dir, tools) = workspace(&[]);
    let names = |work| {
        tools
            .definitions(work)
            .into_iter()
            .map(|tool| tool.function.name)
            .collect::<Vec<_>>()
    };
    assert!(
        !names(crate::durable::work::WorkKind::Single)
            .iter()
            .any(|name| name == "todo")
    );
    assert!(
        names(crate::durable::work::WorkKind::Steps)
            .iter()
            .any(|name| name == "todo")
    );
}

#[test]
fn web_search_is_off_unless_turned_on() {
    let empty = std::collections::BTreeMap::new();
    assert!(
        !crate::modules::Switches::from_maps(&empty, &empty)
            .enabled(crate::modules::ModuleId::Search)
    );
}

#[test]
fn reading_an_image_hands_it_to_a_model_that_sees_images() {
    let (dir, mut tools) = workspace(&[]);
    let mut png = Vec::new();
    ::image::RgbImage::new(2, 2)
        .write_to(
            &mut std::io::Cursor::new(&mut png),
            ::image::ImageFormat::Png,
        )
        .unwrap();
    std::fs::write(dir.path().join("board.png"), &png).unwrap();
    tools.set_images(true, None);
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: serde_json::json!({"path": "board.png"}).to_string(),
        },
        continuation: None,
    };
    let output = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call))
        .unwrap();
    assert_eq!(output.images.len(), 1, "{}", output.text);
    assert!(
        !output.text.contains("shown to the user"),
        "{}",
        output.text
    );
}

#[test]
fn a_file_read_with_context_widens_the_page_around_the_offset() {
    let body: String = (1..2000).map(|n| format!("row{n}\n")).collect();
    let (_dir, tools) = workspace(&[("big.py", &body)]);
    let text = run(
        &tools,
        "read",
        serde_json::json!({"op": "file", "path": "big.py", "offset": 100, "limit": 2, "before": 2, "after": 3}),
    );
    assert!(text.contains("row98") && text.contains("row104"), "{text}");
    assert!(
        !text.contains("row97") && !text.contains("row105"),
        "{text}"
    );
}
