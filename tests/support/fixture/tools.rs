use std::env;
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::Out;

const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
const LINE_WIDTH: usize = 99;
const EXIT_CODE: i32 = 3;

#[derive(Clone, Copy)]
pub(super) enum Tool {
    Echo,
    Write,
    Big,
    Slow,
    Hang,
    Image,
    Fail,
    Stderr,
    Exit,
    Split,
    Deaf,
    Boom,
    Leak,
    Roots,
    Change,
    Progress,
    Log,
    Ask,
    Reprompt,
}

pub(super) struct Row {
    pub(super) tool: Tool,
    pub(super) name: &'static str,
    description: &'static str,
    read_only: bool,
    arg: Option<(&'static str, &'static str)>,
}

pub(super) const TOOLS: &[Row] = &[
    Row {
        tool: Tool::Ask,
        name: "ask",
        description: "Ask the user to fill a form, then return the answer.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Reprompt,
        name: "reprompt",
        description: "Announce that the prompt list changed.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Roots,
        name: "roots",
        description: "Ask the client for its roots and return them.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Change,
        name: "change",
        description: "Announce that the tool list changed.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Progress,
        name: "progress",
        description: "Report progress in steps, then reply.",
        read_only: true,
        arg: Some(("ms", "integer")),
    },
    Row {
        tool: Tool::Log,
        name: "log",
        description: "Send a log message, then reply.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Echo,
        name: "echo",
        description: "Return the given text.",
        read_only: true,
        arg: Some(("text", "string")),
    },
    Row {
        tool: Tool::Write,
        name: "write",
        description: "Pretend to write.",
        read_only: false,
        arg: None,
    },
    Row {
        tool: Tool::Big,
        name: "big",
        description: "Return text of the given byte count.",
        read_only: true,
        arg: Some(("bytes", "integer")),
    },
    Row {
        tool: Tool::Slow,
        name: "slow",
        description: "Sleep the given milliseconds, then reply.",
        read_only: true,
        arg: Some(("ms", "integer")),
    },
    Row {
        tool: Tool::Hang,
        name: "hang",
        description: "Never reply.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Image,
        name: "image",
        description: "Return a 1x1 PNG.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Fail,
        name: "fail",
        description: "Return a tool error.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Stderr,
        name: "stderr",
        description: "Write one line to stderr.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Split,
        name: "split",
        description: "Write half of a big reply, pause, then the rest.",
        read_only: true,
        arg: Some(("bytes", "integer")),
    },
    Row {
        tool: Tool::Deaf,
        name: "deaf",
        description: "Stop reading stdin and never reply.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Boom,
        name: "boom",
        description: "Answer with a JSON-RPC error of the given size.",
        read_only: true,
        arg: Some(("bytes", "integer")),
    },
    Row {
        tool: Tool::Leak,
        name: "leak",
        description: "Print MCP_FIXTURE_SECRET to stderr, then exit.",
        read_only: true,
        arg: None,
    },
    Row {
        tool: Tool::Exit,
        name: "exit",
        description: "Exit the process without a reply.",
        read_only: true,
        arg: None,
    },
];

pub(super) fn advertised() -> Vec<&'static Row> {
    let allowed = env::var("MCP_FIXTURE_TOOLS").ok();
    let names: Option<Vec<&str>> = allowed
        .as_deref()
        .map(|list| list.split(',').map(str::trim).collect());
    TOOLS
        .iter()
        .filter(|row| names.as_ref().is_none_or(|names| names.contains(&row.name)))
        .collect()
}

pub(super) fn describe(row: &Row) -> Value {
    let properties = match row.arg {
        Some((name, kind)) => json!({ name: {"type": kind} }),
        None => json!({}),
    };
    let pad = env::var("MCP_FIXTURE_PAD")
        .ok()
        .and_then(|count| count.parse::<usize>().ok())
        .unwrap_or(0);
    json!({
        "name": row.name,
        "description": format!("{}{}", row.description, " filler".repeat(pad)),
        "inputSchema": {"type": "object", "properties": properties},
        "annotations": {"readOnlyHint": row.read_only},
    })
}

pub(super) fn text(body: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": body.into()}], "isError": false})
}

pub(super) fn big_text(bytes: usize) -> String {
    let mut line = "x".repeat(LINE_WIDTH);
    line.push('\n');
    let mut body = line.repeat(bytes / line.len());
    body.push_str(&line[..bytes % line.len()]);
    body
}

pub(super) fn run(tool: Tool, args: &Value) -> Option<Value> {
    let number = |key: &str| args.get(key).and_then(Value::as_u64).unwrap_or(0);
    match tool {
        Tool::Echo => Some(text(args.get("text").and_then(Value::as_str).unwrap_or(""))),
        Tool::Write => Some(text("wrote")),
        Tool::Big => Some(text(big_text(
            usize::try_from(number("bytes")).unwrap_or(usize::MAX),
        ))),
        Tool::Slow => {
            let ms = number("ms");
            wait(Duration::from_millis(ms));
            Some(text(format!("slow {ms}")))
        }
        Tool::Hang => loop {
            thread::park();
        },
        Tool::Image => Some(json!({
            "content": [
                {"type": "image", "data": PNG_1X1, "mimeType": args.get("mime").and_then(Value::as_str).unwrap_or("image/png")},
                {"type": "text", "text": "image"},
            ],
            "isError": false,
        })),
        Tool::Fail => Some(json!({
            "content": [{"type": "text", "text": "failed on purpose"}],
            "isError": true,
        })),
        Tool::Stderr => {
            eprintln!("fixture stderr line");
            Some(text("stderr"))
        }
        Tool::Exit => process::exit(EXIT_CODE),
        Tool::Leak => {
            eprintln!(
                "token {}",
                env::var("MCP_FIXTURE_SECRET").unwrap_or_default()
            );
            process::exit(EXIT_CODE)
        }
        Tool::Split
        | Tool::Deaf
        | Tool::Boom
        | Tool::Roots
        | Tool::Change
        | Tool::Progress
        | Tool::Log
        | Tool::Ask
        | Tool::Reprompt => None,
    }
}

pub(super) fn wait(pause: Duration) {
    let until = Instant::now() + pause;
    while let Some(left) = until
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
    {
        thread::park_timeout(left);
    }
}

pub(super) fn converse(tool: Tool, out: &Out, id: &Value, params: &Value) {
    match tool {
        Tool::Roots => {
            out.send(&json!({"jsonrpc": "2.0", "id": "fixture-roots", "method": "roots/list"}));
            let roots = super::answer("\"fixture-roots\"");
            out.result(id, text(roots.to_string()));
        }
        Tool::Change => {
            out.send(&json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}));
            out.result(id, text("changed"));
        }
        Tool::Reprompt => {
            out.send(&json!({"jsonrpc": "2.0", "method": "notifications/prompts/list_changed"}));
            out.result(id, text("prompts changed"));
        }
        Tool::Ask => {
            out.send(&json!({"jsonrpc": "2.0", "id": "fixture-ask", "method": "elicitation/create", "params": {
                "mode": "form",
                "message": "Who is asking?",
                "requestedSchema": {
                    "type": "object",
                    "properties": {"name": {"type": "string", "title": "Name"}},
                    "required": ["name"]
                }
            }}));
            let answer = super::answer("\"fixture-ask\"");
            out.result(id, text(answer.to_string()));
        }
        Tool::Progress => {
            let token = params
                .pointer("/_meta/progressToken")
                .cloned()
                .unwrap_or(Value::Null);
            let pause = params
                .pointer("/arguments/ms")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            for step in 1..=4 {
                out.send(&json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": token, "progress": step, "total": 4, "message": format!("step {step}")}}));
                wait(Duration::from_millis(pause));
            }
            out.result(id, text("progressed"));
        }
        Tool::Log => {
            out.send(&json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "warning", "logger": "fixture", "data": "disk low"}}));
            out.result(id, text("logged"));
        }
        Tool::Echo
        | Tool::Write
        | Tool::Big
        | Tool::Slow
        | Tool::Hang
        | Tool::Image
        | Tool::Fail
        | Tool::Stderr
        | Tool::Exit
        | Tool::Split
        | Tool::Deaf
        | Tool::Boom
        | Tool::Leak => {}
    }
}
