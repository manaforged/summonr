use super::*;
use crate::provider::{
    AssistantMessage, CompletionRequest, DEFAULT_BASE_URL, Role, StopReason, ToolCall,
    ToolFunction, ToolKind,
};
use std::future::Future;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

struct FakeProvider {
    rounds: AtomicUsize,
}

struct ImageProvider;

struct TruncatedProvider {
    rounds: AtomicUsize,
}

impl Provider for TruncatedProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            if round == 0 {
                return Ok(AssistantMessage {
                    reasoning_content: Some("long thought".to_string()),
                    stop_reason: StopReason::Length,
                    ..AssistantMessage::default()
                });
            }
            assert!(
                request
                    .messages
                    .last()
                    .is_some_and(|message| { message.content.contains("previous round") })
            );
            assert!(
                request
                    .messages
                    .last()
                    .is_some_and(|message| message.content.starts_with("/no_think"))
            );
            Ok(AssistantMessage {
                content: Some("done".to_string()),
                ..AssistantMessage::default()
            })
        })
    }
}

impl Provider for ImageProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let input = request.messages.last().ok_or_else(|| {
                Error::provider(ProviderFailureKind::Malformed, "missing user input")
            })?;
            assert!(input.content.starts_with("describe\n\nHarness state:"));
            assert_eq!(input.images.len(), 1);
            Ok(AssistantMessage {
                content: Some("done".to_string()),
                ..AssistantMessage::default()
            })
        })
    }
}

impl Provider for FakeProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            assert_eq!(request.model, "test-model");
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            if round == 0 {
                Ok(AssistantMessage {
                    content: None,
                    tool_calls: vec![ToolCall {
                        continuation: None,
                        id: "call-1".to_string(),
                        kind: ToolKind::Function,
                        function: ToolFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path":"note.txt"}"#.to_string(),
                        },
                    }],
                    reasoning_content: Some("inspect the file".to_string()),
                    ..AssistantMessage::default()
                })
            } else {
                let tool = request
                    .messages
                    .last()
                    .map(|message| message.content.clone())
                    .unwrap_or_default();
                Ok(AssistantMessage {
                    content: Some(format!("Read: {tool}")),
                    tool_calls: Vec::new(),
                    reasoning_content: Some("answer from the result".to_string()),
                    ..AssistantMessage::default()
                })
            }
        })
    }
}

fn test_workspace() -> Result<PathBuf> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let workspace = std::env::temp_dir().join(format!(
        "mage-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(workspace.join("note.txt"), "hello\nworld")?;
    Ok(workspace)
}

fn command(unix: &str, windows: &str) -> String {
    serde_json::json!({"command": if cfg!(windows) { windows } else { unix }}).to_string()
}

fn assert_work_receipt(turn: &Turn, provider_text: &str) {
    assert_eq!(turn.text, provider_text);
    assert!(
        turn.events.iter().any(
            |event| matches!(event, Event::Receipt { receipt } if receipt.text.starts_with("Turn receipt"))
        )
    );
}

fn turn_receipt(turn: &Turn) -> &crate::Receipt {
    turn.events
        .iter()
        .find_map(|event| match event {
            Event::Receipt { receipt } => Some(receipt),
            _ => None,
        })
        .expect("turn receipt")
}

fn sandbox_for_test(workspace: &std::path::Path) -> Result<Option<crate::tools::sandbox::Sandbox>> {
    sandbox_for_test_with(workspace, false)
}

fn sandbox_for_test_with(
    workspace: &std::path::Path,
    deny_network: bool,
) -> Result<Option<crate::tools::sandbox::Sandbox>> {
    match crate::tools::sandbox::Sandbox::discover(workspace, deny_network, &[]) {
        Ok(sandbox) => Ok(Some(sandbox)),
        Err(Error::Config(message))
            if message.contains("not available") || message.contains("unsupported") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn shell_arg(path: &std::path::Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn outside_fixture(workspace: &std::path::Path, name: &str) -> PathBuf {
    let id = workspace
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let root = if cfg!(windows) {
        std::env::temp_dir()
    } else {
        PathBuf::from("/var/tmp")
    };
    root.join(format!("mage-sandbox-{name}-{id}"))
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<String> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Ok(String::from_utf8_lossy(&request).into_owned());
        }
        request.extend_from_slice(&buffer[..count]);
        if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while request.len() < header_end + length {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
    }
    Ok(String::from_utf8_lossy(&request[header_end..]).into_owned())
}

fn serve_chat_responses(
    listener: TcpListener,
    bodies: Vec<&'static str>,
) -> std::io::Result<Vec<String>> {
    let mut requests = Vec::new();
    for body in bodies {
        let (mut stream, _) = listener.accept()?;
        requests.push(read_http_request(&mut stream)?);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes())?;
    }
    Ok(requests)
}

#[path = "lib_tests/control_tests.rs"]
mod control_tests;
#[path = "lib_tests/policy_tests.rs"]
mod policy_tests;
#[path = "lib_tests/runtime_tests.rs"]
mod runtime_tests;
#[path = "lib_tests/tools_tests.rs"]
mod tools_tests;
#[test]
fn tagged_text_uses_line_and_hash() {
    let tagged = tagged_text("hello");
    assert!(tagged.starts_with("lines 1-1/1"));
    assert!(tagged.contains(&format!("1:{}|hello", line_hash("hello"))));
}

#[tokio::test]
async fn loop_executes_read_file_tool_and_continues() -> Result<()> {
    let workspace = test_workspace()?;
    let provider = FakeProvider {
        rounds: AtomicUsize::new(0),
    };
    let mut loop_ = Loop::with_session(provider, "test-model", &workspace, "test")?;
    let mut live = Vec::new();
    let turn = loop_
        .turn_with("read the note", |event| live.push(event.clone()))
        .await?;
    assert_eq!(turn.rounds, 2);
    assert!(
        turn.text
            .contains(&format!("1:{}|hello", line_hash("hello")))
    );
    assert!(live.contains(&Event::Unreported { tokens: 0 }));
    live.retain(|event| !matches!(event, Event::Unreported { .. }));
    assert_eq!(live, turn.events);
    let replayed = loop_.replay_session();
    assert!(
        replayed
            .history
            .iter()
            .any(|message| message.role == Role::User && message.content.contains("read the note"))
    );
    assert!(replayed.history.iter().any(|message| {
        message
            .tool_calls
            .iter()
            .any(|call| call.function.name == "read_file")
    }));
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(log.contains("read_file"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn output_limited_reasoning_continues_as_an_append_only_suffix() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        TruncatedProvider {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    let turn = loop_.turn("work").await?;
    assert_work_receipt(&turn, "done");
    assert_eq!(turn.rounds, 2);
    assert!(
        turn.events.iter().any(|event| {
            matches!(event, Event::Note { text } if text.contains("output limit"))
        })
    );
    let replayed = loop_.replay_session();
    assert_eq!(
        replayed
            .history
            .iter()
            .map(|message| message.role)
            .collect::<Vec<_>>(),
        [
            Role::User,
            Role::Assistant,
            Role::User,
            Role::Assistant,
            Role::System
        ]
    );
    assert_eq!(replayed.history[3].content, turn.text);
    assert!(replayed.history[4].content.starts_with("Turn receipt"));
    assert_eq!(replayed.history[1].role, Role::Assistant);
    assert_eq!(
        replayed.history[1].reasoning.as_deref(),
        Some("long thought")
    );
    assert!(replayed.history[2].content.contains("previous round"));
    assert!(replayed.history[2].content.starts_with("/no_think"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn loop_sends_and_persists_image_input() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(ImageProvider, "test-model", &workspace, "test")?;
    let input = UserInput {
        text: "describe".to_string(),
        source: None,
        images: vec![Image {
            media_type: "image/png".to_string(),
            bytes: b"encoded-image".to_vec(),
        }],
    };

    let turn = loop_.turn_with(input, |_| {}).await?;
    assert_work_receipt(&turn, "done");
    let replayed = loop_.replay_session();
    assert_eq!(replayed.history[0].images.len(), 1);

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn image_overflow_does_not_resend_an_unchanged_request() -> Result<()> {
    struct RejectImage {
        calls: Arc<AtomicUsize>,
    }
    impl Provider for RejectImage {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Err(Error::provider(
                    ProviderFailureKind::ContextOverflow,
                    "context length exceeded",
                ))
            })
        }
    }
    let workspace = test_workspace()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut loop_ = Loop::with_session(
        RejectImage {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    let input = UserInput {
        text: "describe".to_string(),
        source: None,
        images: vec![Image {
            media_type: "image/png".to_string(),
            bytes: b"encoded-image".to_vec(),
        }],
    };

    loop_.turn_with(input, |_| {}).await.unwrap_err();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn openai_provider_completes_a_tool_round() -> Result<()> {
    let workspace = test_workspace()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || {
        serve_chat_responses(
            listener,
            vec![
                r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"call-1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"note.txt\"}"}}]}}]}"#,
                r#"{"choices":[{"message":{"content":"done","tool_calls":[]}}]}"#,
            ],
        )
    });
    let provider = Client::new("test-key", format!("http://{address}"))?;
    let mut loop_ = Loop::with_session(provider, "test-model", &workspace, "test")?;
    let turn = loop_.turn("read the note").await?;
    let requests = server
        .join()
        .map_err(|_panic| Error::Tool("provider fixture panicked".to_string()))?
        .map_err(|error| Error::Tool(error.to_string()))?;
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("\"model\":\"test-model\""));
    assert!(requests[0].contains("read_file"));
    assert!(requests[0].contains("replace_text"));
    assert_work_receipt(&turn, "done");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

fn call(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        continuation: None,
        id: "t".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
    }
}

#[cfg(unix)]
#[test]
fn sandbox_write_paths_reach_the_backend() -> Result<()> {
    let workspace = tempfile::tempdir()?;
    let extra = tempfile::tempdir()?;
    let extra_path = extra.path().canonicalize()?;
    let sandbox = match crate::tools::sandbox::Sandbox::discover(
        workspace.path(),
        false,
        std::slice::from_ref(&extra_path),
    ) {
        Ok(sandbox) => sandbox,
        Err(Error::Config(message)) if message.contains("not available") => return Ok(()),
        Err(error) => return Err(error),
    };
    let (_, args) = sandbox.command_line("true", &[], workspace.path());
    let joined = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains(&extra_path.display().to_string()),
        "write path missing from backend arguments:\n{joined}"
    );
    let missing = workspace.path().join("absent");
    assert!(
        crate::tools::sandbox::Sandbox::discover(workspace.path(), false, &[missing]).is_err(),
        "a missing write path must fail discovery"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn sandbox_allow_network_drops_the_denial() -> Result<()> {
    let workspace = tempfile::tempdir()?;
    let sandbox = match crate::tools::sandbox::Sandbox::discover(workspace.path(), true, &[]) {
        Ok(sandbox) => sandbox,
        Err(Error::Config(message))
            if message.contains("not available")
                || message.contains("unsupported")
                || message.contains("preflight failed") =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    assert!(sandbox.denies_network());
    let join = |args: Vec<std::ffi::OsString>| {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let (_, denied) = sandbox.command_line("true", &[], workspace.path());
    let denied = join(denied);
    let allowed = sandbox.allow_network()?;
    assert!(!allowed.denies_network());
    let (_, allowed) = allowed.command_line("true", &[], workspace.path());
    let allowed = join(allowed);
    let marker = if cfg!(target_os = "macos") {
        "(deny network*)"
    } else {
        "--unshare-net"
    };
    assert!(
        denied.contains(marker),
        "denied backend lost its network rule:\n{denied}"
    );
    assert!(
        !allowed.contains(marker),
        "allowed backend still denies network:\n{allowed}"
    );
    Ok(())
}
