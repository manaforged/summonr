use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, duplex, split};
use tokio::time::Duration;

use serde_json::{Value, json};

use super::*;

async fn reply_loop(reader: impl AsyncRead + Unpin, mut writer: impl AsyncWrite + Unpin) {
    let mut reader = BufReader::new(reader);
    while let Ok(msg) = read_msg(&mut reader).await {
        if msg.get("method").and_then(Value::as_str) == Some("exit") {
            break;
        }
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        if msg.get("method").and_then(Value::as_str) == Some("initialize") {
            drop(
                write_msg(
                    &mut writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {} } }),
                )
                .await,
            );
            continue;
        }
        if msg.get("method").and_then(Value::as_str) == Some("textDocument/diagnostic") {
            drop(write_msg(
                &mut writer,
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "kind": "full",
                        "items": [{
                            "message": "missing semicolon",
                            "severity": 1,
                            "range": { "start": { "line": 3, "character": 0 }, "end": { "line": 3, "character": 1 } }
                        }]
                    }
                }),
            )
            .await);
            continue;
        }
        if msg.get("method").and_then(Value::as_str) == Some("textDocument/definition") {
            drop(
                write_msg(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "uri": "file:///tmp/lib.rs",
                            "range": { "start": { "line": 9, "character": 4 } }
                        }
                    }),
                )
                .await,
            );
            continue;
        }
        if msg.get("method").and_then(Value::as_str) == Some("textDocument/references") {
            drop(
                write_msg(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": [
                            {
                                "uri": "file:///tmp/lib.rs",
                                "range": { "start": { "line": 9, "character": 4 } }
                            },
                            {
                                "uri": "file:///tmp/main.rs",
                                "range": { "start": { "line": 0, "character": 7 } }
                            }
                        ]
                    }),
                )
                .await,
            );
            continue;
        }
        if msg.get("method").and_then(Value::as_str) == Some("textDocument/rename") {
            let uri = msg
                .pointer("/params/textDocument/uri")
                .and_then(Value::as_str)
                .unwrap_or("file:///tmp/lib.rs");
            drop(
                write_msg(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "changes": {
                                uri: [{
                                    "range": {
                                        "start": { "line": 0, "character": 3 },
                                        "end": { "line": 0, "character": 7 }
                                    },
                                    "newText": "renamed"
                                }]
                            }
                        }
                    }),
                )
                .await,
            );
            continue;
        }
        if msg.get("method").is_some() {
            drop(
                write_msg(
                    &mut writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                )
                .await,
            );
        }
    }
}

async fn initialization_loop(
    reader: impl AsyncRead + Unpin,
    mut writer: impl AsyncWrite + Unpin,
    mut sender: Option<tokio::sync::oneshot::Sender<Value>>,
) {
    let mut reader = BufReader::new(reader);
    while let Ok(message) = read_msg(&mut reader).await {
        let method = message.get("method").and_then(Value::as_str);
        if method == Some("exit") {
            break;
        }
        if method != Some("initialize") {
            continue;
        }
        sender
            .take()
            .expect("capture initialize once")
            .send(message.clone())
            .expect("capture initialize");
        let id = message.get("id").cloned().expect("initialize id");
        write_msg(
            &mut writer,
            &json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {} } }),
        )
        .await
        .expect("reply initialize");
    }
}

#[tokio::test]
async fn content_length_round_trips() -> Result<()> {
    let (mut writer, reader) = duplex(1024);
    write_msg(&mut writer, &json!({"ok": true})).await?;
    writer.shutdown().await?;
    let mut buf = BufReader::new(reader);
    let value = read_msg(&mut buf).await?;
    assert_eq!(value["ok"], true);
    Ok(())
}

#[tokio::test]
async fn fake_server_reports_diagnostics_and_definition() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("main.rs");
    std::fs::write(&file, "fn main() {}\n")?;
    let (client, server_side) = duplex(64 * 1024);
    let (client_read, client_write) = split(client);
    let (server_read, server_write) = split(server_side);
    let server = tokio::spawn(reply_loop(server_read, server_write));
    let mut conn = Conn::from_io(client_read, client_write, dir.path()).await?;
    let diags = conn.diagnostics(&file).await?;
    assert!(diags.contains("error 4:1: missing semicolon"), "{diags}");
    let def = conn.definition(&file, 1, 1).await?;
    assert!(def.contains("/tmp/lib.rs:10:5"), "{def}");
    let refs = conn.references(&file, 1, 1).await?;
    assert!(refs.contains("/tmp/lib.rs:10:5"), "{refs}");
    assert!(refs.contains("/tmp/main.rs:1:8"), "{refs}");
    let edit = conn.rename(&file, 1, 1, "renamed").await?;
    assert_eq!(
        edit["changes"].as_object().map(serde_json::Map::len),
        Some(1)
    );
    drop(conn);
    drop(server.await);
    Ok(())
}

#[tokio::test]
async fn a_cancelled_tool_drops_the_connection_and_skips_future_checks() -> Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\n",
    )?;
    let file = dir.path().join("main.rs");
    std::fs::write(&file, "fn main() {}\n")?;
    let (client, server_side) = duplex(64 * 1024);
    let (client_read, client_write) = split(client);
    let (server_read, mut server_write) = split(server_side);
    let server = tokio::spawn(async move {
        let mut reader = BufReader::new(server_read);
        let initialize = read_msg(&mut reader).await?;
        let id = initialize["id"].clone();
        write_msg(
            &mut server_write,
            &json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {} } }),
        )
        .await?;
        drop(read_msg(&mut reader).await?);
        drop(read_msg(&mut reader).await?);
        let request = read_msg(&mut reader).await?;
        assert_eq!(request["method"], "textDocument/diagnostic");
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok::<(), Error>(())
    });
    let conn = Conn::from_io(client_read, client_write, dir.path()).await?;
    let tools = ToolRegistry::new(dir.path())?;
    let slot = Arc::new(tokio::sync::Mutex::new(Some(conn)));
    tools
        .lsp
        .lock()
        .expect("lsp map lock")
        .insert("rust".to_string(), slot.clone());

    let started = std::time::Instant::now();
    let output = tools
        .write_file(r#"{"path":"main.rs","content":"fn changed() {}\n"}"#)
        .await?;
    assert!(output.success);
    assert!(output.mutated);
    assert!(output.text.starts_with("wrote main.rs"), "{}", output.text);
    assert!(started.elapsed() < std::time::Duration::from_secs(7));
    assert!(
        slot.lock().await.is_none(),
        "timed out connection was reused"
    );
    assert_eq!(std::fs::read_to_string(&file)?, "fn changed() {}\n");

    let started = std::time::Instant::now();
    let output = tools
        .write_file(r#"{"path":"main.rs","content":"fn changed_again() {}\n"}"#)
        .await?;
    assert!(output.success);
    assert!(output.mutated);
    assert!(!output.text.contains("diagnostics:"), "{}", output.text);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(std::fs::read_to_string(&file)?, "fn changed_again() {}\n");
    server.abort();
    drop(server.await);
    Ok(())
}

#[tokio::test]
async fn initialization_includes_module_options() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let options = language::options(language::named("rust")?);
    let (client, server_side) = duplex(64 * 1024);
    let (client_read, client_write) = split(client);
    let (server_read, server_write) = split(server_side);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(initialization_loop(server_read, server_write, Some(sender)));
    let conn = Conn::from_io_with(client_read, client_write, directory.path(), &options).await?;
    let initialize = receiver
        .await
        .map_err(|error| Error::Tool(format!("cannot receive initialize message: {error}")))?;

    assert_eq!(initialize["params"]["initializationOptions"], options);
    drop(conn);
    server
        .await
        .map_err(|_error| Error::Tool("initialization server panicked".to_string()))?;
    Ok(())
}

#[test]
fn workspace_edit_rewrites_a_workspace_file() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("a.rs");
    std::fs::write(&file, "fn old() {}\n")?;
    let tools = ToolRegistry::new(dir.path())?;
    let uri = format!("file://{}", file.display());
    let mut changes = serde_json::Map::new();
    changes.insert(
        uri,
        json!([{
            "range": {
                "start": { "line": 0, "character": 3 },
                "end": { "line": 0, "character": 6 }
            },
            "newText": "new"
        }]),
    );
    let edit = json!({ "changes": changes });
    let out = tools.apply_workspace_edit(&edit)?;
    assert!(out.mutated);
    assert!(out.text.contains("a.rs"), "{}", out.text);
    assert_eq!(std::fs::read_to_string(&file)?, "fn new() {}\n");
    Ok(())
}

#[test]
fn workspace_edit_is_denied_in_plan_mode() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("a.rs");
    std::fs::write(&file, "fn old() {}\n")?;
    let mut policy = crate::policy::Policy::builtin();
    policy.set_read_only(true);
    let tools = ToolRegistry::with_policy(dir.path(), policy)?;
    let uri = format!("file://{}", file.display());
    let mut changes = serde_json::Map::new();
    changes.insert(
        uri,
        json!([{
            "range": {
                "start": { "line": 0, "character": 3 },
                "end": { "line": 0, "character": 6 }
            },
            "newText": "new"
        }]),
    );
    let edit = json!({ "changes": changes });
    let error = match tools.apply_workspace_edit(&edit) {
        Ok(_) => panic!("rename must fail in plan mode"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("read-only"), "{error}");
    assert_eq!(std::fs::read_to_string(&file)?, "fn old() {}\n");
    Ok(())
}

#[test]
fn workspace_edit_creates_renames_and_deletes() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("a.rs");
    std::fs::write(&src, "fn a() {}\n")?;
    let tools = ToolRegistry::new(dir.path())?;
    let created = dir.path().join("b.rs");
    let edit = json!({
        "documentChanges": [
            { "kind": "create", "uri": format!("file://{}", created.display()) },
            {
                "kind": "rename",
                "oldUri": format!("file://{}", src.display()),
                "newUri": format!("file://{}", dir.path().join("c.rs").display())
            }
        ]
    });
    let out = tools.apply_workspace_edit(&edit)?;
    assert!(out.mutated);
    assert!(created.exists());
    assert!(!src.exists());
    assert!(dir.path().join("c.rs").exists());
    let gone = dir.path().join("c.rs");
    let delete = json!({
        "documentChanges": [
            { "kind": "delete", "uri": format!("file://{}", gone.display()) }
        ]
    });
    tools.apply_workspace_edit(&delete)?;
    assert!(!gone.exists());
    Ok(())
}

#[test]
fn utf16_columns_count_emoji() -> Result<()> {
    let text = "a😀b\n";
    let edits = [TextEdit {
        start_line: 0,
        start_col: 3,
        end_line: 0,
        end_col: 4,
        new_text: "c".to_string(),
    }];
    assert_eq!(apply_text_edits(text, &edits)?, "a😀c\n");
    Ok(())
}

#[test]
fn cargo_workspace_selects_rust_analyzer() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").expect("toml");
    let result = from_tree(dir.path(), &dir.path().join("src/main.rs"));
    match result {
        Ok(launch) => assert_eq!(launch.argv[0], "rust-analyzer"),
        Err(error) => assert!(error.to_string().contains("rust-analyzer")),
    }
}

#[test]
fn js_tree_picks_biome_or_typescript() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(dir.path().join("package.json"), "{}\n").expect("json");
    let result = from_tree(dir.path(), &dir.path().join("app.ts"));
    match result {
        Ok(launch) => assert!(
            launch.argv[0] == "biome" || launch.argv[0] == "typescript-language-server",
            "{:?}",
            launch.argv
        ),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("biome") || text.contains("typescript-language-server"),
                "{text}"
            );
        }
    }
}

#[test]
fn workspace_config_cannot_supply_an_executable() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::create_dir_all(dir.path().join(".mage")).expect("dir");
    std::fs::write(
        dir.path().join(".mage/config.toml"),
        "[lsp]\ncommand = [\"true\"]\n",
    )
    .expect("config");
    let error = detect(dir.path(), &dir.path().join("main.rs"))
        .expect_err("workspace command must fail")
        .to_string();
    assert!(error.contains("cannot set [lsp] command"), "{error}");
}
