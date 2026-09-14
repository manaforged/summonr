use std::future::Future;
use std::pin::Pin;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::provider::{AssistantMessage, Role, ToolFunction, ToolKind};

const CHILDREN: usize = 5;

#[derive(Clone)]
struct Script {
    child: bool,
    parent_calls: Arc<AtomicUsize>,
    child_calls: Arc<AtomicUsize>,
    child_requests: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl Script {
    fn new() -> Self {
        Self {
            child: false,
            parent_calls: Arc::new(AtomicUsize::new(0)),
            child_calls: Arc::new(AtomicUsize::new(0)),
            child_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Provider for Script {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if self.child {
                self.child_requests
                    .lock()
                    .map_err(|_error| Error::Tool("child request lock is poisoned".to_string()))?
                    .push(request.clone());
                return match self.child_calls.fetch_add(1, Ordering::SeqCst) {
                    0 => Ok(message_with_tool(
                        "write_file",
                        r#"{"path":"child.txt","content":"from child\n"}"#,
                        3,
                        2,
                    )),
                    _ => Ok(message("child finished", 4, 1)),
                };
            }
            match self.parent_calls.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(message_with_tool(
                    "task",
                    r#"{"assignment":"write child.txt"}"#,
                    2,
                    1,
                )),
                _ => Ok(message("parent finished", 2, 1)),
            }
        })
    }

    fn child(&self) -> Option<Self> {
        let mut child = self.clone();
        child.child = true;
        Some(child)
    }

    fn supports_children(&self) -> bool {
        true
    }
}

fn message(text: &str, prompt: u32, completion: u32) -> AssistantMessage {
    AssistantMessage {
        content: Some(text.to_string()),
        usage: Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            ..Usage::default()
        },
        ..AssistantMessage::default()
    }
}

fn message_with_tool(
    name: &str,
    arguments: &str,
    prompt: u32,
    completion: u32,
) -> AssistantMessage {
    AssistantMessage {
        tool_calls: vec![ToolCall {
            continuation: None,
            id: format!("{name}-call"),
            kind: ToolKind::Function,
            function: ToolFunction {
                name: name.to_string(),
                arguments: arguments.to_string(),
            },
        }],
        usage: Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            ..Usage::default()
        },
        ..AssistantMessage::default()
    }
}

fn task_call(id: usize, assignment: &str, access: Access) -> ToolCall {
    ToolCall {
        continuation: None,
        id: format!("task-call-{id}"),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "task".to_string(),
            arguments: serde_json::json!({
                "assignment": assignment,
                "access": access,
            })
            .to_string(),
        },
    }
}

fn background_task_call(id: usize, assignment: &str, access: Access) -> ToolCall {
    ToolCall {
        continuation: None,
        id: format!("task-call-{id}"),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "task".to_string(),
            arguments: serde_json::json!({
                "assignment": assignment,
                "access": access,
                "background": true,
            })
            .to_string(),
        },
    }
}

#[tokio::test]
async fn task_runs_one_durable_depth_one_child() -> Result<()> {
    let home = tempfile::tempdir()?;
    let provider = Script::new();
    let requests = provider.child_requests.clone();
    let mut loop_ = Loop::with_session(provider, "test-model", home.path(), "system")?;
    let parent = loop_.session_id();

    let turn = loop_.turn("parent objective").await?;

    assert!(turn.text.starts_with("parent finished"));
    assert_eq!(
        std::fs::read_to_string(home.path().join("child.txt"))?,
        "from child\n"
    );
    assert!(
        loop_
            .work_progress()?
            .touched
            .iter()
            .any(|path| path == "child.txt")
    );
    assert_eq!(turn.usage.prompt_tokens, 11, "parent and child usage");
    assert_eq!(turn.usage.completion_tokens, 5, "parent and child usage");

    let requests = requests
        .lock()
        .map_err(|_error| Error::Tool("child request lock is poisoned".to_string()))?;
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| {
        request.tools.iter().all(|tool| {
            !matches!(
                tool.function.name.as_str(),
                "task" | "web" | "web_search" | "web_fetch"
            )
        })
    }));
    assert!(
        requests[0]
            .messages
            .iter()
            .all(|message| !message.content.contains("parent objective"))
    );

    let sessions = crate::session::list_in(home.path())?;
    assert_eq!(sessions.len(), 2);
    let child = sessions
        .iter()
        .find(|session| session.parent.as_deref() == Some(parent.as_str()))
        .ok_or_else(|| Error::Config("child lineage is missing".to_string()))?;
    let states = log::read(loop_.session.path())?
        .into_iter()
        .filter_map(|line| match line {
            crate::durable::log::Line::Child { record } if record.id.to_string() == child.id => {
                Some(record.state)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        states.as_slice(),
        [ChildState::Running, ChildState::Completed(_)]
    ));
    Ok(())
}

#[tokio::test]
async fn research_child_cannot_write_the_workspace() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut loop_ = Loop::with_session(Script::new(), "test-model", home.path(), "system")?;
    let call = task_call(0, "try to write child.txt", Access::Research);

    let execution = loop_
        .task_execute(&call, &mut Vec::new(), &mut |_: &Event| {})
        .await?;

    assert!(execution.output.success);
    assert!(!execution.output.mutated);
    assert!(!home.path().join("child.txt").exists());
    let record: ChildRecord = serde_json::from_str(&execution.output.text)?;
    assert_eq!(record.access, Access::Research);
    assert!(matches!(
        record.state,
        ChildState::Completed(ChildResult {
            changed_paths_total: 0,
            ..
        })
    ));
    Ok(())
}

#[derive(Clone)]
struct Concurrent {
    child: bool,
    parent_calls: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    arrived: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

impl Concurrent {
    fn new() -> Self {
        Self {
            child: false,
            parent_calls: Arc::new(AtomicUsize::new(0)),
            active: Arc::new(AtomicUsize::new(0)),
            arrived: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Provider for Concurrent {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if !self.child {
                return match self.parent_calls.fetch_add(1, Ordering::SeqCst) {
                    0 => Ok(AssistantMessage {
                        tool_calls: (0..CHILDREN)
                            .map(|index| {
                                task_call(index, &format!("research {index}"), Access::Research)
                            })
                            .collect(),
                        usage: Usage {
                            prompt_tokens: 2,
                            completion_tokens: 1,
                            ..Usage::default()
                        },
                        ..AssistantMessage::default()
                    }),
                    _ => Ok(message("parent finished", 2, 1)),
                };
            }
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            self.arrived.fetch_add(1, Ordering::SeqCst);
            std::future::poll_fn(|context| {
                if self.arrived.load(Ordering::SeqCst) == CHILDREN {
                    std::task::Poll::Ready(())
                } else {
                    context.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            })
            .await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(message("research finished", 2, 1))
        })
    }

    fn child(&self) -> Option<Self> {
        let mut child = self.clone();
        child.child = true;
        Some(child)
    }

    fn supports_children(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn more_than_four_research_children_run_concurrently() -> Result<()> {
    let home = tempfile::tempdir()?;
    let provider = Concurrent::new();
    let peak = provider.peak.clone();
    let mut loop_ = Loop::with_session(provider, "test-model", home.path(), "system")?;

    let turn = loop_.turn("research four questions").await?;

    assert!(turn.text.starts_with("parent finished"));
    assert_eq!(peak.load(Ordering::SeqCst), CHILDREN);
    assert_eq!(loop_.session.children().len(), CHILDREN);
    assert!(loop_.session.children().iter().all(|record| {
        record.access == Access::Research && matches!(record.state, ChildState::Completed(_))
    }));
    Ok(())
}

#[tokio::test]
async fn concurrent_implement_children_run_without_git() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut loop_ = Loop::with_session(Script::new(), "test-model", home.path(), "system")?;
    let calls = vec![
        task_call(0, "write one", Access::Implement),
        task_call(1, "write two", Access::Implement),
    ];

    let results = loop_
        .tasks_execute(&calls, &mut Vec::new(), &mut |_: &Event| {})
        .await;

    assert_eq!(results.len(), 2);
    assert!(results.into_iter().all(|result| result.is_ok()));
    assert_eq!(loop_.session.children().len(), 2);
    Ok(())
}

fn same_workspace(left: &std::path::Path, right: &std::path::Path) -> bool {
    left == right || left.canonicalize().ok() == right.canonicalize().ok()
}

fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "summon@test"],
        vec!["config", "user.name", "Summonr"],
        vec!["config", "core.autocrlf", "false"],
    ] {
        assert!(
            Command::new("git")
                .args(["-C"])
                .arg(dir.path())
                .args(args)
                .status()
                .expect("git")
                .success()
        );
    }
    std::fs::write(dir.path().join("README"), "root\n").expect("README");
    assert!(
        Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(["add", "README"])
            .status()
            .expect("git add")
            .success()
    );
    assert!(
        Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(["commit", "-m", "root"])
            .status()
            .expect("git commit")
            .success()
    );
    dir
}

#[derive(Clone)]
struct Writers {
    child: bool,
    conflict: bool,
    parent_calls: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    arrived: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl Writers {
    fn new() -> Self {
        Self {
            child: false,
            conflict: false,
            parent_calls: Arc::new(AtomicUsize::new(0)),
            active: Arc::new(AtomicUsize::new(0)),
            arrived: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn conflicting() -> Self {
        let mut provider = Self::new();
        provider.conflict = true;
        provider
    }
}

impl Provider for Writers {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if !self.child {
                return match self.parent_calls.fetch_add(1, Ordering::SeqCst) {
                    0 => Ok(AssistantMessage {
                        tool_calls: (0..CHILDREN)
                            .map(|index| {
                                task_call(index, &format!("writer {index}"), Access::Implement)
                            })
                            .collect(),
                        usage: Usage {
                            prompt_tokens: 2,
                            completion_tokens: 1,
                            ..Usage::default()
                        },
                        ..AssistantMessage::default()
                    }),
                    _ => Ok(message("parent finished", 2, 1)),
                };
            }
            self.requests
                .lock()
                .map_err(|_error| Error::Tool("request lock is poisoned".to_string()))?
                .push(request.clone());
            if request
                .messages
                .iter()
                .any(|message| message.role == Role::Tool)
            {
                return Ok(message("implemented", 2, 1));
            }
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            self.arrived.fetch_add(1, Ordering::SeqCst);
            std::future::poll_fn(|context| {
                if self.arrived.load(Ordering::SeqCst) == CHILDREN {
                    std::task::Poll::Ready(())
                } else {
                    context.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            })
            .await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            let index = (0..CHILDREN)
                .find(|index| {
                    request
                        .messages
                        .iter()
                        .any(|message| message.content.contains(&format!("writer {index}")))
                })
                .ok_or_else(|| Error::Tool("writer assignment is missing".to_string()))?;
            let path = if self.conflict {
                "shared.txt".to_string()
            } else {
                format!("child-{index}.txt")
            };
            Ok(message_with_tool(
                "write_file",
                &serde_json::json!({
                    "path": path,
                    "content": format!("child {index}\n"),
                })
                .to_string(),
                2,
                1,
            ))
        })
    }

    fn child(&self) -> Option<Self> {
        let mut child = self.clone();
        child.child = true;
        Some(child)
    }

    fn supports_children(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn more_than_four_implement_children_reconcile_without_leftover_trees() -> Result<()> {
    let workspace = repository();
    let provider = Writers::new();
    let peak = provider.peak.clone();
    let requests = provider.requests.clone();
    let mut loop_ = Loop::with_session(provider, "test-model", workspace.path(), "system")?;

    let turn = loop_.turn("implement four files").await?;

    assert!(turn.text.starts_with("parent finished"));
    assert_eq!(peak.load(Ordering::SeqCst), CHILDREN);
    for index in 0..CHILDREN {
        assert_eq!(
            std::fs::read_to_string(workspace.path().join(format!("child-{index}.txt")))?,
            format!("child {index}\n")
        );
    }
    let records = loop_.session.children();
    assert_eq!(records.len(), CHILDREN);
    assert!(records.iter().all(|record| {
        record.access == Access::Implement
            && matches!(record.state, ChildState::Completed(_))
            && same_workspace(&record.workspace, workspace.path())
    }));
    assert!(crate::worktree::list(workspace.path())?.is_empty());
    let requests = requests
        .lock()
        .map_err(|_error| Error::Tool("request lock is poisoned".to_string()))?;
    assert!(requests.iter().all(|request| {
        request
            .tools
            .iter()
            .any(|tool| tool.function.name == "run_command")
            && request
                .tools
                .iter()
                .all(|tool| tool.function.name != "task")
    }));
    Ok(())
}

#[tokio::test]
async fn implement_children_share_the_parent_and_leave_no_worktrees() -> Result<()> {
    let workspace = repository();
    let mut loop_ = Loop::with_session(
        Writers::conflicting(),
        "test-model",
        workspace.path(),
        "system",
    )?;

    let turn = loop_.turn("implement conflicting files").await?;

    assert!(turn.text.starts_with("parent finished"));
    assert!(workspace.path().join("shared.txt").exists());
    let records = loop_.session.children();
    assert_eq!(records.len(), CHILDREN);
    assert!(records.iter().all(|record| {
        same_workspace(&record.workspace, workspace.path())
            && matches!(record.state, ChildState::Completed(_))
    }));
    assert!(crate::worktree::list(workspace.path())?.is_empty());
    Ok(())
}

#[test]
fn child_budget_is_a_share_of_the_parent_cap_or_unbounded() -> Result<()> {
    let home = tempfile::tempdir()?;
    let loop_ = Loop::with_session(Script::new(), "test-model", home.path(), "system")?;
    let budget = loop_.task_budget(CHILDREN)?;
    assert_eq!(budget.max_tokens, None);
    assert_eq!(budget.max_secs, None);

    let capped = Loop::with_session(Script::new(), "test-model", home.path(), "system")?
        .with_budget(Budget {
            max_tokens: Some(40_000),
            max_secs: Some(60),
        })?;
    let budget = capped.task_budget(CHILDREN)?;
    assert_eq!(budget.max_tokens, Some(40_000 / CHILDREN as u64));
    assert_eq!(budget.max_secs, Some(60));
    Ok(())
}

#[derive(Clone)]
struct BudgetProbe {
    child: bool,
    parent_calls: Arc<AtomicUsize>,
}

impl Provider for BudgetProbe {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if self.child {
                return Ok(message_with_tool(
                    "read_file",
                    r#"{"path":"missing.txt"}"#,
                    6,
                    4,
                ));
            }
            if self.parent_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(AssistantMessage {
                    tool_calls: vec![
                        background_task_call(0, "research one", Access::Research),
                        background_task_call(1, "research two", Access::Research),
                    ],
                    ..AssistantMessage::default()
                });
            }
            Ok(message("parent finished", 0, 0))
        })
    }

    fn child(&self) -> Option<Self> {
        Some(Self {
            child: true,
            parent_calls: self.parent_calls.clone(),
        })
    }

    fn supports_children(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn background_children_share_the_remaining_token_allocation() -> Result<()> {
    let home = tempfile::tempdir()?;
    let provider = BudgetProbe {
        child: false,
        parent_calls: Arc::new(AtomicUsize::new(0)),
    };
    let parent_calls = provider.parent_calls.clone();
    let mut loop_ = Loop::with_session(provider, "test-model", home.path(), "system")?
        .with_budget(Budget {
            max_tokens: Some(15),
            max_secs: None,
        })?;

    let turn = loop_.turn("delegate two children").await?;

    assert!(turn.aborted, "the parent budget must stop after settlement");
    assert_eq!(parent_calls.load(Ordering::SeqCst), 1);
    assert_eq!(turn.usage.prompt_tokens, 12);
    assert_eq!(turn.usage.completion_tokens, 8);
    let children = loop_.session.children();
    assert_eq!(children.len(), 2);
    assert!(children.iter().all(|record| {
        matches!(
            &record.state,
            ChildState::Aborted(result)
                if result.text.contains("7 token limit")
        )
    }));
    Ok(())
}

#[derive(Clone)]
struct MixedAllocationProbe {
    child: bool,
    child_calls: Arc<AtomicUsize>,
}

impl Provider for MixedAllocationProbe {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if !self.child {
                return Ok(message("parent", 0, 0));
            }
            match self.child_calls.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(message_with_tool(
                    "read_file",
                    r#"{"path":"missing.txt"}"#,
                    6,
                    0,
                )),
                1 => Ok(message("foreground finished", 0, 4)),
                2 => Ok(message_with_tool(
                    "read_file",
                    r#"{"path":"missing.txt"}"#,
                    6,
                    0,
                )),
                _ => Ok(message("background finished", 0, 1)),
            }
        })
    }

    fn child(&self) -> Option<Self> {
        Some(Self {
            child: true,
            child_calls: self.child_calls.clone(),
        })
    }

    fn supports_children(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn mixed_task_batches_allocate_background_children_after_foreground_spend() -> Result<()> {
    let home = tempfile::tempdir()?;
    let provider = MixedAllocationProbe {
        child: false,
        child_calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut loop_ = Loop::with_session(provider, "test-model", home.path(), "system")?
        .with_budget(Budget {
            max_tokens: Some(15),
            max_secs: None,
        })?;
    let calls = vec![
        task_call(0, "foreground", Access::Research),
        background_task_call(1, "background", Access::Research),
    ];
    let results = loop_
        .tasks_execute(&calls, &mut Vec::new(), &mut |_: &Event| {})
        .await;
    assert!(results.iter().all(|result| result.is_ok()));
    let foreground = results
        .first()
        .and_then(|result| result.as_ref().ok())
        .map(|execution| execution.usage.clone())
        .ok_or_else(|| Error::Config("foreground result is missing".to_string()))?;
    let mut usage = foreground;
    let mut events = Vec::new();
    let mut emit = |_: &Event| {};
    loop_
        .await_children(&mut events, &mut emit, &mut usage)
        .await?;
    let records = loop_.session.children();
    assert_eq!(records.len(), 2);
    assert!(matches!(
        &records[0].state,
        ChildState::Completed(result) if result.rounds == 2
    ));
    assert!(matches!(
        &records[1].state,
        ChildState::Aborted(result) if result.text.contains("5 token limit")
    ));
    assert_eq!((usage.prompt_tokens, usage.completion_tokens), (12, 4));
    Ok(())
}

#[derive(Clone, Copy)]
enum SettlementMode {
    Automatic,
    Poll,
    Cancel,
    Fail,
}

#[derive(Clone)]
struct SettlementProbe {
    child: bool,
    mode: SettlementMode,
    parent_calls: Arc<AtomicUsize>,
    child_calls: Arc<AtomicUsize>,
}

impl Provider for SettlementProbe {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if !self.child {
                if self.parent_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Ok(AssistantMessage {
                        tool_calls: vec![background_task_call(0, "settle child", Access::Research)],
                        ..AssistantMessage::default()
                    });
                }
                return Ok(message("parent finished", 0, 0));
            }
            match self.mode {
                SettlementMode::Automatic => Ok(message("child finished", 6, 4)),
                SettlementMode::Poll => {
                    tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;
                    Ok(message("child finished", 6, 4))
                }
                SettlementMode::Cancel | SettlementMode::Fail => {
                    if self.child_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        Ok(message_with_tool(
                            "read_file",
                            r#"{"path":"missing.txt"}"#,
                            6,
                            4,
                        ))
                    } else if matches!(self.mode, SettlementMode::Fail) {
                        Err(Error::provider(
                            crate::error::ProviderFailureKind::Http,
                            "fixture provider failure",
                        ))
                    } else {
                        std::future::pending().await
                    }
                }
            }
        })
    }

    fn child(&self) -> Option<Self> {
        Some(Self {
            child: true,
            mode: self.mode,
            parent_calls: self.parent_calls.clone(),
            child_calls: self.child_calls.clone(),
        })
    }

    fn supports_children(&self) -> bool {
        true
    }
}

fn settlement_loop(mode: SettlementMode) -> Result<(tempfile::TempDir, Loop<SettlementProbe>)> {
    let home = tempfile::tempdir()?;
    let provider = SettlementProbe {
        child: false,
        mode,
        parent_calls: Arc::new(AtomicUsize::new(0)),
        child_calls: Arc::new(AtomicUsize::new(0)),
    };
    let loop_ = Loop::with_session(provider, "test-model", home.path(), "system")?;
    Ok((home, loop_))
}

fn child_settlement_call(id: &str, kill: bool) -> ToolCall {
    ToolCall {
        continuation: None,
        id: "settle-child".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "task".to_string(),
            arguments: serde_json::json!({"child": id, "kill": kill}).to_string(),
        },
    }
}

#[tokio::test]
async fn failed_children_keep_reported_usage_separate_from_reservations() -> Result<()> {
    let (_home, mut parent) = settlement_loop(SettlementMode::Fail)?;
    let mut live = Vec::new();
    let turn = parent
        .turn_with("delegate", |event| live.push(event.clone()))
        .await?;
    assert_eq!(
        (turn.usage.prompt_tokens, turn.usage.completion_tokens),
        (6, 4)
    );
    let reserved = parent.unreported_tokens();
    assert!(reserved > 0);
    assert_eq!(parent.spent_tokens(), reserved + 10);
    assert!(
        live.iter()
            .any(|event| matches!(event, Event::Unreported { tokens } if *tokens == reserved))
    );
    let lines = log::read(parent.session_path()).expect("parent journal");
    assert_eq!(lines.iter().filter(|line| matches!(line, log::Line::Charge { reported: true, usage, .. } if usage.input == 6 && usage.output == 4)).count(), 1);
    let result = parent
        .session
        .children()
        .into_iter()
        .find_map(|child| match child.state {
            ChildState::Failed(result) => Some(result),
            _ => None,
        })
        .expect("failed child receipt");
    assert_eq!(
        (result.usage.prompt_tokens, result.usage.completion_tokens),
        (6, 4)
    );
    Ok(())
}

#[tokio::test]
async fn child_usage_is_reported_once_for_automatic_poll_and_cancel_settlement() -> Result<()> {
    let (_automatic_home, mut automatic) = settlement_loop(SettlementMode::Automatic)?;
    let automatic_turn = automatic.turn("delegate").await?;
    assert_eq!(
        (
            automatic_turn.usage.prompt_tokens,
            automatic_turn.usage.completion_tokens
        ),
        (6, 4)
    );

    let (_polled_home, mut polled) = settlement_loop(SettlementMode::Poll)?;
    let first = polled.turn("delegate").await?;
    assert!(!first.aborted);
    let child = polled
        .session
        .children()
        .first()
        .ok_or_else(|| Error::Config("poll child is missing".to_string()))?
        .id
        .to_string();
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let polled_execution = polled
        .task_execute(
            &child_settlement_call(&child, false),
            &mut Vec::new(),
            &mut |_: &Event| {},
        )
        .await?;
    assert_eq!(
        (
            polled_execution.usage.prompt_tokens,
            polled_execution.usage.completion_tokens
        ),
        (6, 4)
    );

    let (_cancelled_home, mut cancelled) = settlement_loop(SettlementMode::Cancel)?;
    let first = cancelled.turn("delegate").await?;
    assert!(!first.aborted);
    let child = cancelled
        .session
        .children()
        .first()
        .ok_or_else(|| Error::Config("cancel child is missing".to_string()))?
        .id
        .to_string();
    let cancelled_execution = cancelled
        .task_execute(
            &child_settlement_call(&child, true),
            &mut Vec::new(),
            &mut |_: &Event| {},
        )
        .await?;
    assert_eq!(
        (
            cancelled_execution.usage.prompt_tokens,
            cancelled_execution.usage.completion_tokens
        ),
        (6, 4)
    );
    Ok(())
}

#[tokio::test]
async fn cancelled_task_batch_removes_clean_child_worktrees() -> Result<()> {
    let workspace = repository();
    let provider = Pending {
        child: false,
        parent_calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut loop_ = Loop::with_session(provider, "test-model", workspace.path(), "system")?;
    let calls = vec![
        task_call(0, "wait forever", Access::Implement),
        task_call(1, "wait forever", Access::Implement),
    ];
    let mut events = Vec::new();
    let mut emit = |_: &Event| {};

    let outcome = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        loop_.tasks_execute(&calls, &mut events, &mut emit),
    )
    .await;

    assert!(outcome.is_err(), "the batch was cancelled by the timeout");
    assert!(crate::worktree::list(workspace.path())?.is_empty());
    Ok(())
}

#[tokio::test]
async fn child_events_stream_to_the_parent_tagged_with_the_child_id() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut loop_ = Loop::with_session(Script::new(), "test-model", home.path(), "system")?;
    let mut seen = Vec::new();

    let turn = loop_
        .turn_with("parent objective", |event| seen.push(event.clone()))
        .await?;

    let child = loop_.session.children();
    let id = child
        .first()
        .map(|record| record.id.to_string())
        .ok_or_else(|| Error::Config("no child".to_string()))?;
    let names: Vec<&str> = seen
        .iter()
        .filter_map(|event| match event {
            Event::Child { id: from, event } if *from == id => match &**event {
                Event::ToolCall { name, .. } => Some(name.as_str()),
                Event::Assistant { .. } => Some("assistant"),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(names, ["write_file", "assistant"]);
    assert!(turn.events.iter().any(|event| matches!(
        event,
        Event::Child { id: from, .. } if *from == id
    )));
    Ok(())
}

#[derive(Clone)]
struct Pending {
    child: bool,
    parent_calls: Arc<AtomicUsize>,
}

impl Provider for Pending {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if self.child {
                return std::future::pending().await;
            }
            self.parent_calls.fetch_add(1, Ordering::SeqCst);
            Ok(message_with_tool(
                "task",
                r#"{"assignment":"wait forever"}"#,
                1,
                1,
            ))
        })
    }

    fn child(&self) -> Option<Self> {
        let mut child = self.clone();
        child.child = true;
        Some(child)
    }

    fn supports_children(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn task_deadline_aborts_and_journals_the_child() -> Result<()> {
    let home = tempfile::tempdir()?;
    let provider = Pending {
        child: false,
        parent_calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut loop_ = Loop::with_session(provider, "test-model", home.path(), "system")?
        .with_budget(Budget {
            max_tokens: Some(1_000),
            max_secs: Some(1),
        })?;

    let turn = loop_.turn("delegate").await?;

    assert!(
        turn.aborted,
        "the parent deadline also stops the next round"
    );
    let states = log::read(loop_.session.path())?
        .into_iter()
        .filter_map(|line| match line {
            crate::durable::log::Line::Child { record } => Some(record.state),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        states.as_slice(),
        [
            ChildState::Running,
            ChildState::Aborted(_) | ChildState::Interrupted(_)
        ]
    ));
    Ok(())
}

#[tokio::test]
async fn task_accepts_a_large_assignment() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut loop_ = Loop::with_session(Script::new(), "test-model", home.path(), "system")?;
    let call = ToolCall {
        continuation: None,
        id: "task-call".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "task".to_string(),
            arguments: serde_json::json!({
                "assignment": "x".repeat(8_000)
            })
            .to_string(),
        },
    };

    let output = loop_
        .task_execute(&call, &mut Vec::new(), &mut |_: &Event| {})
        .await?;
    assert!(output.output.success);
    assert_eq!(crate::session::list_in(home.path())?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn child_tools_keep_commands_and_inherit_restrictions() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut parent =
        crate::tools::ToolRegistry::with_policy_and_search(home.path(), Policy::builtin(), None)?;
    parent.set_strict();
    parent.set_read_only(true);
    let child = parent.child_at(home.path(), "child")?;

    assert!(child.is_strict());
    assert!(child.is_read_only());
    assert!(child.definitions().iter().all(|tool| {
        !matches!(
            tool.function.name.as_str(),
            "web" | "web_search" | "web_fetch"
        )
    }));
    assert!(
        child
            .definitions()
            .iter()
            .any(|tool| tool.function.name == "run_command")
    );
    let writable =
        crate::tools::ToolRegistry::with_policy_and_search(home.path(), Policy::builtin(), None)?
            .child_at(home.path(), "child")?;
    let ok = ToolCall {
continuation: None,
        id: "command-call".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": if cfg!(windows) { "[Console]::Write('ran')" } else { "printf ran" }}).to_string(),
        },
    };
    let output = writable.execute_outcome(&ok).await?;
    assert!(output.success);
    assert!(output.text.contains("ran"));
    let background = ToolCall {
        continuation: None,
        id: "background-call".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "run_command".to_string(),
            arguments: r#"{"command":"sleep 5","background":true}"#.to_string(),
        },
    };
    let error = match writable.execute_outcome(&background).await {
        Err(error) => error,
        Ok(_) => {
            return Err(Error::Config(
                "child background command unexpectedly started".to_string(),
            ));
        }
    };
    assert!(error.to_string().contains("unavailable in child sessions"));
    Ok(())
}

fn last_tool(request: &CompletionRequest) -> String {
    request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == Role::Tool)
        .map(|message| message.content.clone())
        .unwrap_or_default()
}

fn child_id(text: &str) -> Option<String> {
    let first = text.lines().next()?;
    first
        .strip_prefix("started child ")
        .or_else(|| first.strip_prefix("child "))
        .map(str::to_string)
}

fn poll_call(id: &str, kill: bool) -> AssistantMessage {
    message_with_tool(
        "task",
        &serde_json::json!({"child": id, "kill": kill}).to_string(),
        1,
        1,
    )
}

#[derive(Clone)]
struct Background {
    child: bool,
    hang: bool,
    parent_calls: Arc<AtomicUsize>,
    child_calls: Arc<AtomicUsize>,
}

impl Background {
    fn new(hang: bool) -> Self {
        Self {
            child: false,
            hang,
            parent_calls: Arc::new(AtomicUsize::new(0)),
            child_calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Provider for Background {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if self.child {
                if self.hang {
                    return std::future::pending().await;
                }
                return match self.child_calls.fetch_add(1, Ordering::SeqCst) {
                    0 => Ok(message_with_tool(
                        "write_file",
                        r#"{"path":"child.txt","content":"from child\n"}"#,
                        3,
                        2,
                    )),
                    _ => Ok(message("child finished", 4, 1)),
                };
            }
            let last = last_tool(request);
            let call = self.parent_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                return Ok(message_with_tool(
                    "task",
                    r#"{"assignment":"write child.txt","background":true}"#,
                    2,
                    1,
                ));
            }
            if last.contains("\"status\":\"completed\"") || last.contains("\"status\":\"aborted\"")
            {
                return Ok(message("parent finished", 2, 1));
            }
            let id = child_id(&last).ok_or_else(|| Error::Tool("no child id".to_string()))?;
            if self.hang {
                return Ok(poll_call(&id, true));
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Ok(poll_call(&id, false))
        })
    }

    fn child(&self) -> Option<Self> {
        let mut child = self.clone();
        child.child = true;
        Some(child)
    }

    fn supports_children(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn background_child_is_polled_reconciled_and_streamed() -> Result<()> {
    let workspace = repository();
    let provider = Background::new(false);
    let calls = provider.parent_calls.clone();
    let mut loop_ = Loop::with_session(provider, "test-model", workspace.path(), "system")?;
    let mut seen = Vec::new();

    let turn = loop_
        .turn_with("delegate in the background", |event| {
            seen.push(event.clone())
        })
        .await?;

    assert!(turn.text.starts_with("parent finished"));
    assert!(calls.load(Ordering::SeqCst) >= 3, "start, poll, finish");
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("child.txt"))?,
        "from child\n"
    );
    assert!(crate::worktree::list(workspace.path())?.is_empty());
    let records = loop_.session.children();
    assert_eq!(records.len(), 1);
    assert!(matches!(records[0].state, ChildState::Completed(_)));
    assert!(seen.iter().any(|event| matches!(
        event,
        Event::Child { event, .. } if matches!(**event, Event::ToolCall { .. })
    )));
    assert!(
        loop_
            .work_progress()?
            .touched
            .iter()
            .any(|path| path == "child.txt")
    );
    Ok(())
}

#[tokio::test]
async fn background_child_can_be_killed() -> Result<()> {
    let workspace = repository();
    let mut loop_ = Loop::with_session(
        Background::new(true),
        "test-model",
        workspace.path(),
        "system",
    )?;

    let turn = loop_.turn("delegate then stop").await?;

    assert!(turn.text.starts_with("parent finished"));
    let records = loop_.session.children();
    assert_eq!(records.len(), 1);
    assert!(matches!(
        &records[0].state,
        ChildState::Aborted(result)
            if result.diagnostic.as_deref().is_some_and(|text| text.contains("killed"))
    ));
    assert!(crate::worktree::list(workspace.path())?.is_empty());
    Ok(())
}

#[derive(Clone)]
struct Leaver {
    child: bool,
    parent_calls: Arc<AtomicUsize>,
}

impl Provider for Leaver {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if self.child {
                return std::future::pending().await;
            }
            match self.parent_calls.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(message_with_tool(
                    "task",
                    r#"{"assignment":"wait forever","access":"research","background":true}"#,
                    2,
                    1,
                )),
                _ => Ok(message("done", 2, 1)),
            }
        })
    }

    fn child(&self) -> Option<Self> {
        let mut child = self.clone();
        child.child = true;
        Some(child)
    }

    fn supports_children(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn branching_keeps_uncollected_background_children_running() -> Result<()> {
    let home = tempfile::tempdir()?;
    let provider = Leaver {
        child: false,
        parent_calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut loop_ = Loop::with_session(provider, "test-model", home.path(), "system")?;
    drop(
        loop_
            .task_execute(
                &background_task_call(0, "wait", Access::Research),
                &mut Vec::new(),
                &mut |_: &Event| {},
            )
            .await?,
    );
    assert_eq!(loop_.spawned.len(), 1);
    let parent = loop_.session_id();
    let journal = std::fs::read(loop_.session_path())?;
    for turn in [None, Some(0)] {
        let error = loop_
            .fork_at(turn)
            .expect_err("uncollected child prevents branching");
        assert!(error.to_string().contains("background agents"));
        assert_eq!(loop_.session_id(), parent);
        assert_eq!(loop_.spawned.len(), 1);
        assert!(
            loop_
                .session
                .children()
                .iter()
                .all(|record| matches!(record.state, ChildState::Running))
        );
        assert_eq!(std::fs::read(loop_.session_path())?, journal);
    }
    let id = loop_.session.children()[0].id.to_string();
    drop(
        loop_
            .task_execute(
                &child_settlement_call(&id, true),
                &mut Vec::new(),
                &mut |_: &Event| {},
            )
            .await?,
    );
    Ok(())
}

#[tokio::test]
async fn turns_defer_while_a_background_child_runs_until_explicitly_stopped() -> Result<()> {
    let home = tempfile::tempdir()?;
    let provider = Leaver {
        child: false,
        parent_calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut loop_ = Loop::with_session(provider, "test-model", home.path(), "system")?;

    let first = loop_.turn("delegate and leave").await?;
    let deferred = first.events.iter().any(|event| {
        matches!(event, Event::Note { text } if text.starts_with("turn deferred") && text.contains("child"))
    });
    assert!(deferred, "{:?}", first.events);
    assert!(
        loop_
            .session
            .children()
            .iter()
            .all(|record| matches!(record.state, ChildState::Running))
    );

    for _ in 0..5 {
        drop(loop_.turn("continue").await?);
    }

    let records = loop_.session.children();
    assert_eq!(records.len(), 1);
    assert!(matches!(&records[0].state, ChildState::Running));
    let id = records[0].id.to_string();
    let call = ToolCall {
        continuation: None,
        id: "stop-child".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "task".to_string(),
            arguments: serde_json::json!({"child": id, "kill": true}).to_string(),
        },
    };
    drop(
        loop_
            .task_execute(&call, &mut Vec::new(), &mut |_: &Event| {})
            .await?,
    );
    let records = loop_.session.children();
    assert!(
        matches!(&records[0].state, ChildState::Aborted(_)),
        "{records:?}"
    );
    Ok(())
}
