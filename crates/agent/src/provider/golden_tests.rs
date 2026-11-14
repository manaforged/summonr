use std::fmt::Write;
use std::path::{Path, PathBuf};

use super::cache_tests::endpoint;
use super::{
    Client, CompletionRequest, Message, Role, ToolCall, ToolDefinition, ToolFunction, ToolKind,
    ToolsJson,
};

const BLESS: &str = "SUMMONR_BLESS";
const SESSION: &str = "golden-session";
const SYSTEM: &str = "You are summonr, a coding agent. Réponds brièvement — ✓";
const CALL: &str = "call_read_1";
const ARGUMENTS: &str = r#"{"path":"config/ré.toml","range":{"start":0}}"#;
const READ_SCHEMA: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"Chemin relatif — UTF-8"},"range":{"type":"object","description":"Lines to read","properties":{"start":{"type":"integer","default":0},"end":{"type":"integer"}},"required":["start"]}},"required":["path"]}"#;
const SAMPLE_SCHEMA: &str = r#"{"type":"object","properties":{"temperature":{"type":"number","default":0.5},"epsilon":{"type":"number","default":1e-7},"top_p":{"type":"number","default":0.1},"mode":{"type":"string","enum":["greedy","nucleus","échantillon"]},"nested":{"type":"object","properties":{"seed":{"type":"integer"},"label":{"type":"string","default":"día"}}}},"required":["mode"]}"#;
const FLOATS: [&str; 3] = [r#""default":0.5"#, r#""default":1e-7"#, r#""default":0.1"#];

struct Case {
    wire: &'static str,
    endpoint: &'static str,
    model: &'static str,
}

const CASES: [Case; 4] = [
    Case {
        wire: "anthropic",
        endpoint: "anthropic",
        model: "claude-opus-5-5",
    },
    Case {
        wire: "completions",
        endpoint: "deepseek",
        model: "deepseek-flash",
    },
    Case {
        wire: "responses",
        endpoint: "openai",
        model: "gpt-6-astra",
    },
    Case {
        wire: "gemini",
        endpoint: "gemini",
        model: "gemini-3.8-flash",
    },
];

fn tools() -> Vec<ToolDefinition> {
    [
        (
            "read_file",
            "Read a file — returns UTF-8 text.",
            READ_SCHEMA,
        ),
        (
            "set_sampling",
            "Tune sampling: température, top_p.",
            SAMPLE_SCHEMA,
        ),
    ]
    .into_iter()
    .map(|(name, description, schema)| {
        summonr_types::tool(name, description, serde_json::from_str(schema).unwrap())
    })
    .collect()
}

fn assistant(text: &str, tool_calls: Vec<ToolCall>) -> Message {
    let mut message = Message::user(text);
    message.role = Role::Assistant;
    message.tool_calls = tool_calls;
    message
}

fn history() -> Vec<Message> {
    let read = ToolCall {
        id: CALL.to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read_file".to_string(),
            arguments: ARGUMENTS.to_string(),
        },
        continuation: None,
    };
    vec![
        Message::user("Lis config/ré.toml and set the sampling — 日本語 ok?"),
        assistant("Reading it first.", vec![read]),
        Message::tool(CALL, "temperature = 0.5\nepsilon = 1e-7 # naïve"),
        assistant(
            "The file sets temperature 0.5. Shall I lower it?",
            Vec::new(),
        ),
        Message::user("Yes — use 0.1 ✓"),
    ]
}

struct Sent {
    body: Vec<u8>,
    prefix: String,
}

fn sent(case: &Case) -> Sent {
    let mut endpoint = endpoint(case.endpoint, case.model);
    endpoint.oauth = None;
    let messages = history();
    let tools = tools();
    let json = ToolsJson::encode(&tools);
    let request = CompletionRequest::new(
        SESSION.to_string(),
        case.model.to_string(),
        &messages,
        &tools,
        None,
        true,
        None,
    )
    .with_system_prompt(SYSTEM)
    .with_tools_json(&json);
    let prefix = request.prefix_hash();
    let request = request.with_prefix_hash(prefix.clone());
    let body = Client::connect(&endpoint)
        .unwrap()
        .request(&request, true)
        .unwrap()
        .to_bytes()
        .unwrap();
    Sent { body, prefix }
}

fn suffix() -> &'static str {
    let probe = serde_json::json!({"b": 0, "a": 0});
    if probe
        .as_object()
        .and_then(|map| map.keys().next())
        .map(String::as_str)
        == Some("a")
    {
        ""
    } else {
        ".ordered"
    }
}

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/wire")
}

fn around(bytes: &[u8], at: usize) -> String {
    let start = at.saturating_sub(80);
    let end = (at + 80).min(bytes.len());
    String::from_utf8_lossy(&bytes[start..end]).into_owned()
}

fn clear(written: &Path) {
    if written.exists() {
        std::fs::remove_file(written).unwrap();
    }
}

fn golden(name: &str, actual: &[u8]) -> Option<String> {
    let path = dir().join(name);
    let written = dir().join(format!("{name}.actual"));
    if std::env::var_os(BLESS).is_some() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(&path, actual).unwrap();
        clear(&written);
        return None;
    }
    let held = std::fs::read(&path).unwrap_or_default();
    if held == actual {
        clear(&written);
        return None;
    }
    std::fs::write(&written, actual).unwrap();
    let at = held
        .iter()
        .zip(actual)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| held.len().min(actual.len()));
    let (held_path, written_path) = (path.display(), written.display());
    let review = if name.ends_with(".json") {
        format!("diff <(jq . {held_path}) <(jq . {written_path})")
    } else {
        format!("diff {held_path} {written_path}")
    };
    Some(format!(
        "{name} drifted at byte {at} (golden {} bytes, actual {} bytes)\n  golden: …{}…\n  actual: …{}…\n  review: {review}\n  accept: {BLESS}=1",
        held.len(),
        actual.len(),
        around(&held, at),
        around(actual, at),
    ))
}

#[test]
fn every_wire_sends_the_pinned_request_bytes() {
    let suffix = suffix();
    let mut drift = Vec::new();
    let mut hashes = String::new();
    for case in &CASES {
        let sent = sent(case);
        writeln!(hashes, "{} {}", case.wire, sent.prefix).unwrap();
        drift.extend(golden(&format!("{}{suffix}.json", case.wire), &sent.body));
    }
    drift.extend(golden(
        &format!("prefix-hash{suffix}.txt"),
        hashes.as_bytes(),
    ));
    assert!(drift.is_empty(), "{}", drift.join("\n\n"));
}

#[test]
fn schema_floats_reach_every_wire_as_written() {
    for case in &CASES {
        let body = sent(case).body;
        let text = String::from_utf8_lossy(&body);
        for token in FLOATS {
            assert!(SAMPLE_SCHEMA.contains(token), "the fixture lost {token}");
            assert!(
                text.contains(token),
                "{} rewrote {token} on the wire: {text}",
                case.wire
            );
        }
    }
}
