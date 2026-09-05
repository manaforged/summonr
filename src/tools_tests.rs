use super::*;

impl ToolRegistry {
    pub(crate) fn new(workspace: impl AsRef<Path>) -> Result<Self> {
        Self::with_policy_and_search(workspace, Policy::builtin(), None)
    }

    pub(crate) fn with_policy(workspace: impl AsRef<Path>, policy: Policy) -> Result<Self> {
        Self::with_policy_and_search(workspace, policy, None)
    }
}

#[test]
fn attachments_separate_images_from_text() -> Result<()> {
    let workspace = std::env::temp_dir().join(format!(
        "mage-attach-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| Error::Tool(error.to_string()))?
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(workspace.join("note.txt"), "hello")?;
    std::fs::write(workspace.join("pixel.png"), b"\x89PNG\r\n\x1a\n")?;

    let input = attach(&workspace, "compare @note.txt @pixel.png")?;
    assert!(input.text.starts_with("compare\n"));
    assert!(input.text.contains("--- note.txt ---\nhello"));
    assert_eq!(input.images.len(), 1);
    assert_eq!(input.images[0].media_type, "image/png");

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[test]
fn attachments_reject_files_outside_the_workspace() -> Result<()> {
    let base = std::env::temp_dir().join(format!("mage-attach-boundary-{}", std::process::id()));
    let workspace = base.join("workspace");
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(base.join("outside.png"), b"\x89PNG\r\n\x1a\n")?;

    let error = attach(&workspace, "look @../outside.png")
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(error.contains("outside the workspace"));

    std::fs::remove_dir_all(base)?;
    Ok(())
}

#[test]
fn attachments_limit_total_image_bytes() -> Result<()> {
    let workspace = std::env::temp_dir().join(format!("mage-attach-size-{}", std::process::id()));
    std::fs::create_dir_all(&workspace)?;
    let mut image = vec![0_u8; 6 * 1024 * 1024];
    image[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    std::fs::write(workspace.join("one.png"), &image)?;
    std::fs::write(workspace.join("two.png"), &image)?;

    let error = attach(&workspace, "@one.png @two.png")
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(error.contains("attached images are larger"));

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn child_tools_rebuild_the_parent_sandbox_for_the_child_workspace() -> Result<()> {
    let root = tempfile::tempdir_in(std::env::current_dir()?)?;
    let parent_workspace = root.path().join("parent");
    let child_workspace = root.path().join("child");
    let outside = root.path().join("outside.txt");
    std::fs::create_dir_all(&parent_workspace)?;
    std::fs::create_dir_all(&child_workspace)?;
    let mut parent = ToolRegistry::new(&parent_workspace)?;
    if parent.enable_sandbox(true).is_err() {
        return Ok(());
    }
    parent.set_unrestricted();
    parent.set_modules(false, false, true, false);

    let child = parent.child_at(&child_workspace, "child")?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let write = ToolCall {
        id: "write".to_string(),
        kind: ToolKind::Function,
        function: crate::provider::ToolFunction {
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": format!("printf blocked > '{}'", outside.display())
            })
            .to_string(),
        },
    };
    let result = child.execute_outcome(&write).await?;
    let network = ToolCall {
        id: "network".to_string(),
        kind: ToolKind::Function,
        function: crate::provider::ToolFunction {
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": format!(
                    "python3 -c \"import socket; socket.create_connection(('127.0.0.1', {port}), 1)\""
                )
            })
            .to_string(),
        },
    };
    let network = child.execute_outcome(&network).await?;

    assert!(!result.success, "{}", result.text);
    assert!(!outside.exists());
    assert!(!network.success, "{}", network.text);
    assert!(child.definitions().iter().all(|tool| {
        !matches!(
            tool.function.name.as_str(),
            "lsp_definition" | "lsp_references" | "lsp_symbols"
        )
    }));
    Ok(())
}

#[tokio::test]
async fn foreground_commands_kill_successful_and_failed_descendants() -> Result<()> {
    for exit in [0, 7] {
        let workspace = tempfile::tempdir()?;
        let tools = ToolRegistry::new(workspace.path())?;
        let command = format!("(sleep 1; printf late > late.txt) & exit {exit}");
        let call = ToolCall {
            id: format!("exit-{exit}"),
            kind: ToolKind::Function,
            function: crate::provider::ToolFunction {
                name: "run_command".to_string(),
                arguments: serde_json::json!({ "command": command }).to_string(),
            },
        };

        let result = tools.execute_outcome(&call).await?;
        assert_eq!(result.success, exit == 0, "{}", result.text);
        tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;
        assert!(!workspace.path().join("late.txt").exists());
    }
    Ok(())
}

#[tokio::test]
async fn stopping_session_jobs_does_not_stop_another_session() -> Result<()> {
    let workspace = tempfile::tempdir()?;
    let first = ToolRegistry::new(workspace.path())?;
    let second = ToolRegistry::new(workspace.path())?;
    first.set_session("first");
    second.set_session("second");
    let start = |id: &str| ToolCall {
        id: id.to_string(),
        kind: ToolKind::Function,
        function: crate::provider::ToolFunction {
            name: "run_command".to_string(),
            arguments: r#"{"command":"sleep 30","background":true}"#.to_string(),
        },
    };
    let first_id = first
        .execute(&start("first-job"))
        .await?
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .ok_or_else(|| Error::Tool("missing first job id".to_string()))?
        .to_string();
    let second_id = second
        .execute(&start("second-job"))
        .await?
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .ok_or_else(|| Error::Tool("missing second job id".to_string()))?
        .to_string();

    first.stop_jobs()?;
    assert!(crate::jobs::status(workspace.path(), &first_id)?.killed);
    assert!(crate::jobs::status(workspace.path(), &second_id)?.running);

    second.stop_jobs()?;
    assert!(crate::jobs::status(workspace.path(), &second_id)?.killed);
    Ok(())
}
