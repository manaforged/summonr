use serde_json::{Value, json};

use crate::support::{Model, Session, Step, advertised, calls};

fn large(session: &Session) -> String {
    format!(
        "{}{}",
        session.server_env("fixture1", ", MCP_FIXTURE_PAD = '200'", ""),
        session.server_env("fixture2", ", MCP_FIXTURE_TOOLS = 'echo'", "")
    )
}

pub(crate) fn a_large_server_is_found_and_called_through_the_mcp_tool() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("mcp", json!({"query": "echo"})),
        Step::Call(
            "mcp",
            json!({"tool": "fixture1__echo", "arguments": {"text": "deep"}}),
        ),
        Step::Text("ok"),
    ]);
    session.run(&model, &large(&session), &[]);
    let tools = advertised(&model.requests()[0]);
    assert!(tools.contains(&"mcp".to_string()), "{tools:?}");
    assert!(tools.contains(&"fixture2__echo".to_string()), "{tools:?}");
    assert!(
        !tools.iter().any(|tool| tool.starts_with("fixture1__")),
        "{tools:?}"
    );
    let results = model.tool_results();
    let text: String = serde_json::from_str(&results[0]).unwrap_or_else(|_| results[0].clone());
    let found: Value = serde_json::from_str(text.lines().next().unwrap_or_default())
        .unwrap_or_else(|_| panic!("{results:?}"));
    assert!(
        found["tool"]
            .as_str()
            .is_some_and(|tool| tool.ends_with("__echo")),
        "{found}"
    );
    assert_eq!(
        found["input_schema"]["properties"]["text"]["type"],
        "string"
    );
    assert!(results[1].contains("deep"), "{results:?}");
    let received = session.received("fixture1");
    let sent = calls(&received, "tools/call");
    assert_eq!(sent.len(), 1, "{received:?}");
    assert_eq!(sent[0]["params"]["name"], json!("echo"));
    assert_eq!(sent[0]["params"]["arguments"], json!({"text": "deep"}));
    let printed = crate::support::read(&session.file("stderr"));
    assert!(printed.contains("→ mcp fixture1:echo"), "{printed}");
}

#[cfg(unix)]
const CAPTURE: &str = "cat > '{0}'";
#[cfg(windows)]
const CAPTURE: &str = "[IO.File]::WriteAllText('{0}', [Console]::In.ReadToEnd())";

pub(crate) fn a_hook_sees_the_tool_behind_the_mcp_tool() {
    let session = Session::new();
    let seen = session.file("pre-tool.json");
    let hook = CAPTURE.replace("{0}", &seen.display().to_string());
    let config = format!(
        "[hooks]\npre_tool = {}\n{}",
        serde_json::to_string(&hook).unwrap(),
        large(&session)
    );
    let model = Model::start(vec![
        Step::Call(
            "mcp",
            json!({"tool": "fixture1__echo", "arguments": {"text": "hooked"}}),
        ),
        Step::Text("ok"),
    ]);
    session.run(&model, &config, &[]);
    let input: Value = serde_json::from_str(&std::fs::read_to_string(&seen).unwrap()).unwrap();
    assert_eq!(input["tool"], json!("fixture1__echo"), "{input}");
    assert_eq!(input["arguments"], json!({"text": "hooked"}), "{input}");
}
