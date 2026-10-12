use std::sync::Arc;

use super::*;

const ASK: &str = "Run the shell command `sleep 2 && echo ready` with run_command, then reply with one short sentence.";
const STEER: &str = "Also: end your final reply with the word PINEAPPLE.";

async fn steered(name: &str) -> std::result::Result<String, String> {
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
        .turn_with(UserInput::from(ASK), |event| {
            if !sent && matches!(event, Event::ToolCall { .. }) {
                steering.lock().unwrap().push_back(UserInput::from(STEER));
                sent = true;
            }
        })
        .await
        .map_err(|error| error.to_string())?;
    if !sent {
        return Err(format!("no tool call: {}", turn.text));
    }
    Ok(turn.text)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "calls the endpoints named in SUMMONR_LIVE_STEER with their stored keys"]
async fn a_steer_sent_while_a_tool_runs_changes_the_live_models_answer() {
    let names = std::env::var("SUMMONR_LIVE_STEER").unwrap_or_default();
    let mut failed = Vec::new();
    for name in names.split(',').filter(|name| !name.is_empty()) {
        match steered(name).await {
            Ok(text) if text.contains("PINEAPPLE") => println!("{name}: steered: {text}"),
            Ok(text) => failed.push(format!("{name}: ignored the steer: {text}")),
            Err(error) => failed.push(format!("{name}: {error}")),
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
