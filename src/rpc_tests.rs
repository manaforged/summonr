use super::*;

#[test]
fn frames_classify_by_method_and_id() {
    let request =
        parse(r#"{"jsonrpc":"2.0","id":7,"method":"ping","params":{"a":1}}"#).expect("request");
    assert_eq!(
        request,
        Frame::Request {
            id: json!(7),
            method: "ping".to_string(),
            params: json!({"a": 1}),
        }
    );
    let note = parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).expect("note");
    assert_eq!(
        note,
        Frame::Notification {
            method: "notifications/initialized".to_string(),
            params: Value::Null,
        }
    );
    let reply = parse(r#"{"jsonrpc":"2.0","id":"x","result":{"ok":true}}"#).expect("reply");
    assert_eq!(
        reply,
        Frame::Response {
            id: json!("x"),
            result: Some(json!({"ok": true})),
            error: None,
        }
    );
    drop(parse(r#"{"jsonrpc":"2.0"}"#).expect_err("no method or id"));
    drop(parse("not json").expect_err("not json"));
}

#[test]
fn text_joins_content_parts() {
    let result = json!({"content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]});
    assert_eq!(text(&result), "a\nb");
    assert_eq!(text(&json!({})), "");
}

#[tokio::test]
async fn client_calls_a_stdio_server_and_routes_notifications() {
    let script = r#"
import sys, json
for line in sys.stdin:
    m = json.loads(line)
    if m.get("method") == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":m["id"],"result":{"protocolVersion":"2025-03-26","capabilities":{}}}), flush=True)
    elif m.get("method") == "tools/call":
        print(json.dumps({"jsonrpc":"2.0","method":"progress","params":{"n":1}}), flush=True)
        print(json.dumps({"jsonrpc":"2.0","id":99,"method":"server/ask","params":{}}), flush=True)
        print(json.dumps({"jsonrpc":"2.0","id":m["id"],"result":{"content":[{"type":"text","text":"echo " + m["params"]["arguments"]["say"]}]}}), flush=True)
    elif m.get("method") == "boom":
        print(json.dumps({"jsonrpc":"2.0","id":m["id"],"error":{"code":-1,"message":"no"}}), flush=True)
"#;
    let dir = std::env::temp_dir().join(format!("summon-rpc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("server.py");
    std::fs::write(&path, script).expect("script");
    let mut client = Client::spawn("python3", &[&path.display().to_string()], &dir).expect("spawn");
    client.initialize("test").await.expect("initialize");
    let mut notes = Vec::new();
    let result = client
        .tool("echo", json!({"say": "hi"}), |method, params| {
            notes.push((method.to_string(), params.clone()));
        })
        .await
        .expect("tool");
    assert_eq!(text(&result), "echo hi");
    assert_eq!(notes, vec![("progress".to_string(), json!({"n": 1}))]);
    let error = client
        .call("boom", json!({}), |_, _| {})
        .await
        .expect_err("error");
    assert!(error.to_string().contains("`boom` failed: no"));
    client.shutdown().await;
    drop(std::fs::remove_dir_all(&dir));
}
