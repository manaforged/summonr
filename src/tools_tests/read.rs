use super::*;

#[test]
fn todo_is_advertised_from_the_first_request_so_a_goal_keeps_the_prompt_cache() {
    let (_dir, tools) = workspace(&[]);
    let names: Vec<String> = tools
        .definitions()
        .into_iter()
        .map(|tool| tool.function.name)
        .collect();
    assert!(names.iter().any(|name| name == "todo"), "{names:?}");
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
fn a_child_registry_inherits_the_web_modules() {
    let (dir, mut tools) = workspace(&[]);
    tools.search = Some(crate::web::Search::discover().unwrap());
    let child = tools.child_at(dir.path(), "child").unwrap();
    let names: Vec<String> = child
        .definitions()
        .into_iter()
        .map(|tool| tool.function.name)
        .collect();
    assert!(names.iter().any(|name| name == "web"), "{names:?}");
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

#[test]
fn a_batch_read_keeps_line_tags_for_every_file() {
    let (_dir, tools) = workspace(&[("a.txt", "one\n"), ("b.txt", "two\n")]);
    let call = ToolCall {
        id: "t".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: serde_json::json!({ "paths": ["a.txt", "b.txt"], "tags": true }).to_string(),
        },
        continuation: None,
    };
    let text = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call))
        .unwrap()
        .text;
    let tagged = |line: &str| {
        text.lines()
            .any(|row| row.starts_with("1:") && row.contains('|') && row.ends_with(line))
    };
    assert!(tagged("one") && tagged("two"), "{text}");
}

#[test]
fn a_read_reaches_a_file_outside_the_workspace_by_absolute_or_relative_path() {
    let (dir, tools) = workspace(&[]);
    let other = tempfile::tempdir_in(dir.path().parent().unwrap()).unwrap();
    let file = other.path().join("notes.md");
    std::fs::write(&file, "kept outside\n").unwrap();
    let name = other.path().file_name().unwrap().to_string_lossy();
    for path in [file.display().to_string(), format!("../{name}/notes.md")] {
        let text = run(&tools, "read", serde_json::json!({ "path": path }));
        assert!(text.contains("kept outside"), "{path}: {text}");
    }
}

#[cfg(unix)]
#[test]
fn a_protected_name_outside_the_workspace_stays_protected_through_a_symlink() {
    let (_dir, tools) = workspace(&[]);
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("plain.txt"), "API_KEY=1\n").unwrap();
    let link = other.path().join(".env");
    std::os::unix::fs::symlink(other.path().join("plain.txt"), &link).unwrap();
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: serde_json::json!({"path": link.display().to_string()}).to_string(),
        },
        continuation: None,
    };
    let error = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call))
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(error.contains("protected path"), "{error}");
}

#[test]
fn a_directory_listing_gives_file_sizes_in_bytes() {
    let (_dir, tools) = workspace(&[("calc.py", "print('ninety-eight bytes of code')\n")]);
    let listing = run(&tools, "read", serde_json::json!({"path": "."}));
    assert!(listing.contains("calc.py\tfile\t36 bytes"), "{listing}");
}
