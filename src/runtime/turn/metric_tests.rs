use super::*;
use crate::provider::scripted::{Reply, Scripted};
use crate::provider::{ToolCall, ToolFunction, ToolKind};

struct Run {
    loop_: Loop<Scripted>,
    workspace: std::path::PathBuf,
    sent: crate::provider::scripted::Sent,
    _dirs: [tempfile::TempDir; 2],
}

fn write(path: &str, content: &str) -> Reply {
    Reply::Call(ToolCall {
        id: "w".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "write".to_string(),
            arguments: serde_json::json!({ "path": path, "content": content }).to_string(),
        },
        continuation: None,
    })
}

fn attempt(path: &str, content: &str) -> [Reply; 2] {
    [write(path, content), Reply::Text("tried it")]
}

fn metric_goal(check_file: &str, replies: Vec<Reply>) -> Run {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let root = workspace.path().to_path_buf();
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::write(root.join("score"), "10\n").unwrap();
    std::fs::create_dir_all(root.join(".summonr")).unwrap();
    std::fs::write(root.join(".summonr/check.toml"), check_file).unwrap();
    let driver = super::super::configure::open_driver(home.path(), &root, "m", "system").unwrap();
    let provider = Scripted::new(replies);
    let sent = provider.sent();
    let mut loop_ = Loop::open(provider, "m", &root, "system", driver).unwrap();
    let commands = crate::verify::check_commands(&root).unwrap().unwrap();
    loop_.set_verify(Some(commands.clone())).unwrap();
    loop_.select_goal("raise the score", &commands).unwrap();
    Run {
        loop_,
        workspace: root,
        sent,
        _dirs: [workspace, home],
    }
}

fn score(run: &Run) -> String {
    std::fs::read_to_string(run.workspace.join("score")).unwrap()
}

const CLIMB: &str =
    "[metric]\ncommand = \"cat score\"\nbetter = \"higher\"\nnoise = 0.5\ntarget = 100\n";

#[tokio::test]
async fn a_metric_goal_keeps_only_turns_that_beat_the_best_by_more_than_the_noise() {
    let replies = [
        attempt("score", "12\n"),
        attempt("score", "11\n"),
        attempt("score", "12.3\n"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut run = metric_goal(CLIMB, replies);
    run.loop_.turn("raise the score").await.unwrap();
    assert_eq!(score(&run), "12\n");
    run.loop_.turn("continue").await.unwrap();
    assert_eq!(score(&run), "12\n");
    run.loop_.turn("continue").await.unwrap();
    assert_eq!(score(&run), "12\n");
    let progress = run.loop_.work_progress().unwrap();
    assert_eq!(
        progress.ledger,
        [
            "#1 10 baseline",
            "#2 12 kept",
            "#3 11 reverted",
            "#4 12.3 reverted"
        ]
    );
    assert!(!progress.terminal());
}

#[tokio::test]
async fn a_metric_goal_ends_when_the_kept_value_reaches_the_target() {
    let check = "[metric]\ncommand = \"cat score\"\nbetter = \"lower\"\ntarget = 4\n";
    let mut run = metric_goal(check, attempt("score", "3.5\n").into());
    let turn = run.loop_.turn("lower the score").await.unwrap();
    assert!(turn.events.iter().any(
        |event| matches!(event, Event::Verify { command, passed: true } if command == "cat score")
    ));
    assert_eq!(score(&run), "3.5\n");
    assert_eq!(run.loop_.work_progress().unwrap().exit_code(), Some(0));
}

#[tokio::test]
async fn a_failed_check_reverts_the_turn_and_repeated_failures_do_not_end_a_metric_goal() {
    let check = format!("check = \"test ! -e broken\"\n{CLIMB}");
    let replies = [attempt("broken", "x\n"), attempt("broken", "x\n")]
        .into_iter()
        .flatten()
        .collect();
    let mut run = metric_goal(&check, replies);
    run.loop_.turn("raise the score").await.unwrap();
    assert!(!run.workspace.join("broken").exists());
    run.loop_.turn("continue").await.unwrap();
    assert!(!run.workspace.join("broken").exists());
    let progress = run.loop_.work_progress().unwrap();
    assert_eq!(
        progress.ledger,
        [
            "#1 10 baseline",
            "#2 no value reverted",
            "#3 no value reverted"
        ]
    );
    assert!(!progress.terminal(), "{:?}", progress.reason);
}

#[tokio::test]
async fn a_first_turn_that_regresses_from_the_unchanged_workspace_is_reverted() {
    let mut run = metric_goal(CLIMB, attempt("score", "5\n").into());
    run.loop_.turn("raise the score").await.unwrap();
    assert_eq!(score(&run), "10\n");
    assert_eq!(
        run.loop_.work_progress().unwrap().ledger,
        ["#1 10 baseline", "#2 5 reverted"]
    );
}

#[tokio::test]
async fn a_turn_that_edits_a_protected_file_is_reverted_without_running_the_metric() {
    let check =
        "[metric]\ncommand = \"sh bench.sh\"\nbetter = \"higher\"\nprotect = [\"bench.sh\"]\n";
    let mut run = metric_goal(check, attempt("bench.sh", "echo 999\n").into());
    std::fs::write(run.workspace.join("bench.sh"), "cat score\n").unwrap();
    let commands = crate::verify::check_commands(&run.workspace)
        .unwrap()
        .unwrap();
    run.loop_.set_verify(Some(commands)).unwrap();
    run.loop_.turn("raise the score").await.unwrap();
    assert_eq!(
        std::fs::read_to_string(run.workspace.join("bench.sh")).unwrap(),
        "cat score\n"
    );
    assert_eq!(
        run.loop_.work_progress().unwrap().ledger,
        ["#1 10 baseline", "#2 no value reverted"]
    );
}

#[tokio::test]
async fn the_model_is_told_the_metric_command_the_noise_bar_and_the_revert_before_it_works() {
    let mut run = metric_goal(CLIMB, attempt("score", "12\n").into_iter().collect());
    run.loop_.turn("raise the score").await.unwrap();
    let sent = run.sent.lock().unwrap();
    let first = sent.first().expect("the turn sent a request");
    let told = first.iter().any(|message| {
        message
            .content
            .contains("runs `cat score`; higher is better")
            && message.content.contains("beats the best by more than 0.5")
            && message
                .content
                .contains("restores every file to the start of the turn")
            && message.content.contains("ends when the best reaches 100")
    });
    assert!(
        told,
        "the first request did not state the metric rules: {first:?}"
    );
}
