use super::*;
use crate::provider::scripted::{Reply, Scripted};
use crate::provider::{ToolFunction, ToolKind};

const NUDGE: &str = "[summon control: repeated tool results.";

fn command(id: usize, command: &str) -> Reply {
    Reply::Call(ToolCall {
        id: format!("t{id}"),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({ "command": command }).to_string(),
        },
        continuation: None,
    })
}

async fn results(turns: &[Vec<&str>]) -> Vec<Message> {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut replies = Vec::new();
    for commands in turns {
        let base = replies.len();
        replies.extend(
            commands
                .iter()
                .enumerate()
                .map(|(id, text)| command(base + id, text)),
        );
        replies.push(Reply::Text("done"));
    }
    let provider = Scripted::new(replies);
    let sent = provider.sent();
    let driver =
        super::super::configure::open_driver(home.path(), workspace.path(), "m", "system").unwrap();
    let mut loop_ = Loop::open(provider, "m", workspace.path(), "system", driver).unwrap();
    for _ in turns {
        loop_.turn("go").await.unwrap();
    }
    let sent = sent.lock().unwrap();
    sent.last()
        .unwrap()
        .iter()
        .filter(|message| message.role == Role::Tool)
        .cloned()
        .collect()
}

fn nudged(results: &[Message]) -> Vec<bool> {
    results
        .iter()
        .map(|message| message.content.contains(NUDGE))
        .collect()
}

fn repeats() -> usize {
    crate::limits::limits().repeat_nudge
}

#[tokio::test]
async fn the_same_call_and_result_gets_one_nudge_on_the_last_repeat_and_still_runs() {
    let results = results(&[vec!["echo same"; repeats()]]).await;
    let mut expected = vec![false; repeats()];
    *expected.last_mut().unwrap() = true;
    assert_eq!(nudged(&results), expected, "{results:?}");
    assert!(
        results
            .iter()
            .all(|message| message.content.contains("same"))
    );
}

#[tokio::test]
async fn the_same_call_with_a_changing_result_is_never_nudged() {
    let results = results(&[vec!["echo x >> n.txt; cat n.txt"; repeats()]]).await;
    assert_eq!(nudged(&results), vec![false; repeats()], "{results:?}");
}

#[tokio::test]
async fn an_alternating_pair_of_calls_is_nudged_after_the_pair_repeats() {
    let pair = ["echo a", "echo b"];
    let commands: Vec<&str> = pair.iter().copied().cycle().take(2 * repeats()).collect();
    let results = results(&[commands]).await;
    let mut expected = vec![false; 2 * repeats()];
    *expected.last_mut().unwrap() = true;
    assert_eq!(nudged(&results), expected, "{results:?}");
}

#[tokio::test]
async fn a_new_user_message_starts_the_count_again() {
    let results = results(&[vec!["echo same"; repeats() - 1], vec!["echo same"]]).await;
    assert_eq!(nudged(&results), vec![false; repeats()], "{results:?}");
}
