use std::sync::{Arc, Mutex};

use super::*;
use crate::provider::scripted::{Reply, Scripted};
use crate::provider::{Source, ToolFunction, ToolKind};

const ASK: &str = "Run the shell command `sleep 2 && echo ready` with run_command, then reply with one short sentence.";
const STEER: &str = "Also: end your final reply with the word PINEAPPLE.";

const LONG_ASK: &str = "Run the shell command `sleep 2` with run_command three times, one call at a time, then reply with one short sentence.";
const QUESTION: &str = "whats the deal? are we having issues?";

struct Live {
    text: String,
    records: Vec<serde_json::Value>,
}

async fn steered(name: &str, ask: &str, input: UserInput) -> std::result::Result<Live, String> {
    let endpoint = crate::endpoint::resolve(Some(name), None, crate::Need::Key)
        .map_err(|error| error.to_string())?;
    let client = crate::provider::Client::connect(&endpoint).map_err(|error| error.to_string())?;
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let driver = super::super::configure::open_driver(
        home.path(),
        workspace.path(),
        &endpoint.model,
        "system",
    )
    .unwrap();
    let mut loop_ = Loop::open(
        client,
        &endpoint.model,
        workspace.path(),
        "You are a coding agent. Use the tools when asked.",
        driver,
    )
    .unwrap();
    let steering = Arc::new(Steering::default());
    loop_.steer_with(Arc::clone(&steering));
    let mut sent = false;
    let turn = loop_
        .turn_with(UserInput::from(ask), |event| {
            if !sent && matches!(event, Event::ToolCall { .. }) {
                steering.lock().unwrap().push_back(input.clone());
                sent = true;
            }
        })
        .await
        .map_err(|error| error.to_string())?;
    if !sent {
        return Err(format!("no tool call: {}", turn.text));
    }
    let mut records = Vec::new();
    for entry in std::fs::read_dir(crate::durable::log::sessions_dir(home.path())).unwrap() {
        let journal = std::fs::read_to_string(entry.unwrap().path().join("session.jsonl")).unwrap();
        records.extend(
            journal
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok()),
        );
    }
    Ok(Live {
        text: turn.text,
        records,
    })
}

fn next_reply(records: &[serde_json::Value]) -> Option<&serde_json::Value> {
    let kind = |record: &serde_json::Value, name: &str| record["kind"] == name;
    records
        .iter()
        .position(|record| kind(record, "user") && record["start"] == false)
        .and_then(|at| {
            records[at..]
                .iter()
                .find(|record| kind(record, "assistant"))
        })
}

fn answered_next(name: &str, arm: &str, records: &[serde_json::Value]) -> bool {
    let reply = next_reply(records);
    let said = |text: &serde_json::Value| text.as_str().is_some_and(|text| !text.trim().is_empty());
    let answered = reply.is_some_and(|reply| {
        said(&reply["text"])
            || reply["continuation"]["commentary"]
                .as_array()
                .is_some_and(|remarks| remarks.iter().any(said))
    });
    if !answered {
        println!("{name} {arm}: next reply {reply:?}");
    }
    answered
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "calls the endpoints named in SUMMONR_LIVE_STEER with env keys when SUMMONR_LIVE_KEYS is set"]
async fn a_steer_sent_while_a_tool_runs_changes_the_live_models_answer() {
    let names = std::env::var("SUMMONR_LIVE_STEER").unwrap_or_default();
    let mut failed = Vec::new();
    for name in names.split(',').filter(|name| !name.is_empty()) {
        match steered(name, ASK, UserInput::from(STEER)).await {
            Ok(live) if live.text.contains("PINEAPPLE") => {
                println!("{name}: steered: {}", live.text)
            }
            Ok(live) => failed.push(format!("{name}: ignored the steer: {}", live.text)),
            Err(error) => failed.push(format!("{name}: {error}")),
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "calls the endpoints named in SUMMONR_LIVE_STEER with env keys when SUMMONR_LIVE_KEYS is set"]
async fn a_live_model_answers_a_question_typed_mid_turn() {
    let names = std::env::var("SUMMONR_LIVE_STEER").unwrap_or_default();
    let trials: usize = std::env::var("SUMMONR_LIVE_TRIALS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3);
    let mut failed = Vec::new();
    for name in names.split(',').filter(|name| !name.is_empty()) {
        let mut typed_answers = 0;
        let mut plain_answers = 0;
        for _ in 0..trials {
            match steered(name, LONG_ASK, typed(QUESTION)).await {
                Ok(live) => {
                    typed_answers += usize::from(answered_next(name, "note", &live.records))
                }
                Err(error) => failed.push(format!("{name}: {error}")),
            }
            match steered(name, LONG_ASK, UserInput::from(QUESTION)).await {
                Ok(live) => {
                    plain_answers += usize::from(answered_next(name, "plain", &live.records))
                }
                Err(error) => failed.push(format!("{name}: {error}")),
            }
        }
        println!(
            "{name}: answered next with the note {typed_answers}/{trials}, without {plain_answers}/{trials}"
        );
        if typed_answers < trials {
            failed.push(format!(
                "{name}: answered the typed question in {typed_answers}/{trials} trials"
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

fn typed(text: &str) -> UserInput {
    UserInput {
        text: text.to_string(),
        images: Vec::new(),
        source: Some(Source {
            text: text.to_string(),
            images: 0,
        }),
    }
}

async fn request_after_steer(input: UserInput) -> Vec<Message> {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let steering = Arc::new(Steering::default());
    let late = Arc::clone(&steering);
    let pending = Mutex::new(Some(input));
    let call = Reply::Call(ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({ "command": "echo hi" }).to_string(),
        },
        continuation: None,
    });
    let provider = Scripted::new([call, Reply::Text("ok")]).on_complete(move |call| {
        if let (0, Some(input)) = (call, pending.lock().unwrap().take()) {
            late.lock().unwrap().push_back(input);
        }
    });
    let sent = provider.sent();
    let driver =
        super::super::configure::open_driver(home.path(), workspace.path(), "m", "system").unwrap();
    let mut loop_ = Loop::open(provider, "m", workspace.path(), "system", driver).unwrap();
    loop_.steer_with(steering);
    loop_
        .turn_with(UserInput::from("list the files"), |_| {})
        .await
        .unwrap();
    let sent = sent.lock().unwrap();
    sent[1].clone()
}

fn steered_content(request: &[Message], text: &str) -> String {
    request
        .iter()
        .find(|message| message.content.starts_with(text))
        .map(|message| message.content.clone())
        .unwrap_or_else(|| panic!("the steer never reached the model: {request:?}"))
}

#[tokio::test]
async fn a_typed_steer_reaches_the_model_with_a_reply_note() {
    let request = request_after_steer(typed("why is the gap still open")).await;
    assert_eq!(
        steered_content(&request, "why is the gap still open"),
        "why is the gap still open\n\nHarness: the user sent this while you were working. Answer it."
    );
}

#[tokio::test]
async fn a_steer_the_user_did_not_type_reaches_the_model_unchanged() {
    let request = request_after_steer(UserInput::from("approved")).await;
    assert_eq!(steered_content(&request, "approved"), "approved");
}
