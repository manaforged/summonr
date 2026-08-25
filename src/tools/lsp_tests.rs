use std::io::{BufReader, Read, Write, pipe};
use std::thread;

use serde_json::{Value, json};

use super::*;

fn reply_loop(reader: impl Read, mut writer: impl Write) {
    let mut reader = BufReader::new(reader);
    while let Ok(msg) = read_msg(&mut reader) {
        if msg.get("method").and_then(Value::as_str) == Some("exit") {
            break;
        }
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        if msg.get("method").and_then(Value::as_str) == Some("initialize") {
            drop(write_msg(
                &mut writer,
                &json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {} } }),
            ));
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
            ));
            continue;
        }
        if msg.get("method").and_then(Value::as_str) == Some("textDocument/definition") {
            drop(write_msg(
                &mut writer,
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "uri": "file:///tmp/lib.rs",
                        "range": { "start": { "line": 9, "character": 4 } }
                    }
                }),
            ));
            continue;
        }
        if msg.get("method").and_then(Value::as_str) == Some("textDocument/references") {
            drop(write_msg(
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
            ));
            continue;
        }
        if msg.get("method").and_then(Value::as_str) == Some("textDocument/rename") {
            let uri = msg
                .pointer("/params/textDocument/uri")
                .and_then(Value::as_str)
                .unwrap_or("file:///tmp/lib.rs");
            drop(write_msg(
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
            ));
            continue;
        }
        if msg.get("method").is_some() {
            drop(write_msg(
                &mut writer,
                &json!({ "jsonrpc": "2.0", "id": id, "result": null }),
            ));
        }
    }
}

#[test]
fn content_length_round_trips() -> Result<()> {
    let (reader, mut writer) = pipe()?;
    write_msg(&mut writer, &json!({"ok": true}))?;
    drop(writer);
    let mut buf = BufReader::new(reader);
    let value = read_msg(&mut buf)?;
    assert_eq!(value["ok"], true);
    Ok(())
}

#[test]
fn fake_server_reports_diagnostics_and_definition() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("main.rs");
    std::fs::write(&file, "fn main() {}\n")?;
    let (server_read, client_write) = pipe()?;
    let (client_read, server_write) = pipe()?;
    let server = thread::spawn(move || reply_loop(server_read, server_write));
    let mut conn = Conn::from_io(client_read, client_write, dir.path())?;
    let diags = conn.diagnostics(&file)?;
    assert!(diags.contains("error 4:1: missing semicolon"), "{diags}");
    let def = conn.definition(&file, 1, 1)?;
    assert!(def.contains("/tmp/lib.rs:10:5"), "{def}");
    let refs = conn.references(&file, 1, 1)?;
    assert!(refs.contains("/tmp/lib.rs:10:5"), "{refs}");
    assert!(refs.contains("/tmp/main.rs:1:8"), "{refs}");
    let edit = conn.rename(&file, 1, 1, "renamed")?;
    assert_eq!(
        edit["changes"].as_object().map(serde_json::Map::len),
        Some(1)
    );
    drop(conn);
    drop(server.join());
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
    let result = from_tree(dir.path());
    match result {
        Ok(argv) => assert_eq!(argv[0], "rust-analyzer"),
        Err(error) => assert!(error.to_string().contains("rust-analyzer")),
    }
}

#[test]
fn js_tree_picks_biome_or_typescript() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(dir.path().join("package.json"), "{}\n").expect("json");
    let result = from_tree(dir.path());
    match result {
        Ok(argv) => assert!(
            argv[0] == "biome" || argv[0] == "typescript-language-server",
            "{argv:?}"
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
fn workspace_config_plugs_in_the_server() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::create_dir_all(dir.path().join(".mage")).expect("dir");
    std::fs::write(
        dir.path().join(".mage/config.toml"),
        "[lsp]\ncommand = [\"true\"]\n",
    )
    .expect("config");
    let argv = detect(dir.path()).expect("detect");
    assert_eq!(argv, ["true"]);
}
