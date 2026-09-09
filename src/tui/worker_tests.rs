use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::unbounded_channel;

use super::*;
use crate::prefs::Prefs;

fn worker() -> (Worker, UnboundedSender<Cmd>, PathBuf) {
    let (worker, tx, _ui, workspace) = worker_ui();
    (worker, tx, workspace)
}

fn worker_ui() -> (
    Worker,
    UnboundedSender<Cmd>,
    UnboundedReceiver<Msg>,
    PathBuf,
) {
    let workspace = std::env::temp_dir().join(format!(
        "mage-worker-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap_or_else(|error| panic!("{error}"));
    let loop_ = Loop::with_session(
        Client::new("", "http://localhost/v1").unwrap_or_else(|error| panic!("{error}")),
        "test",
        &workspace,
        "system",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let (tx, rx) = unbounded_channel();
    let (ui, ui_rx) = unbounded_channel();
    let info = Info {
        version: "test".to_string(),
        model: "test".to_string(),
        relay: None,
        session: "test".to_string(),
        endpoint: "test".to_string(),
        prefs: Prefs::default(),
        plan: false,
        strict: false,
        workspace: workspace.display().to_string(),
        workflow: None,
        config: None,
    };
    (Worker::new(loop_, rx, ui, info), tx, ui_rx, workspace)
}

#[tokio::test]
async fn saves_finish_in_command_order_before_shutdown() {
    let (worker, tx, mut ui, workspace) = worker_ui();
    let file = workspace.join("notes.txt");
    tx.send(Cmd::Save(file.clone(), "old".repeat(100_000)))
        .expect("first save");
    tx.send(Cmd::Save(file.clone(), "new".to_string()))
        .expect("second save");
    tx.send(Cmd::Quit).expect("quit");
    worker.run().await;
    let mut saved = Vec::new();
    while let Ok(msg) = ui.try_recv() {
        if let Msg::Saved { text, result, .. } = msg {
            result.expect("save succeeded");
            saved.push(text);
        }
    }
    assert_eq!(saved, vec!["old".repeat(100_000), "new".to_string()]);
    assert_eq!(std::fs::read_to_string(file).expect("saved file"), "new");
    std::fs::remove_dir_all(workspace).expect("scratch workspace");
}

#[tokio::test]
async fn cancelled_login_stops_polling_and_returns_pending_input() {
    const CASE: &str = "SUMMON_LOGIN_TEST";
    if std::env::var_os(CASE).is_none() {
        let home = tempfile::tempdir().expect("isolated home");
        let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "tui::worker::tests::cancelled_login_stops_polling_and_returns_pending_input",
                "--exact",
            ])
            .env_clear()
            .env(CASE, "1")
            .env("MAGE_HOME", home.path())
            .status()
            .expect("isolated login test");
        assert!(status.success());
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let config = format!(
        "[endpoint.fixture]\nmodel = 'fixture'\nbase_url = 'http://{address}'\n[endpoint.fixture.auth]\ngrant = 'device_code'\nclient_id = 'fixture'\ndevice_url = 'http://{address}/device'\ntoken_url = 'http://{address}/token'\n"
    );
    std::fs::write(crate::config::path().expect("config path"), config).expect("config");
    let (mut worker, tx, mut ui, workspace) = worker_ui();
    let serve = async {
        let (mut connection, _) = listener.accept().await.expect("device request");
        let mut request = [0; 4096];
        let count = connection.read(&mut request).await.expect("request");
        assert!(String::from_utf8_lossy(&request[..count]).starts_with("POST /device "));
        let body = r#"{"device_code":"fixture-device","user_code":"FIXTURE","verification_uri":"https://example.invalid/device","expires_in":60,"interval":1}"#;
        connection.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.expect("device response");
    };
    let cancel = async {
        while let Some(message) = ui.recv().await {
            if matches!(message, Msg::System(text) if text == "code FIXTURE") {
                tx.send(Cmd::Queue("keep this draft".into()))
                    .expect("queue");
                tx.send(Cmd::SetRounds(3)).expect("setting");
                tx.send(Cmd::Cancel).expect("cancel");
                return;
            }
        }
        panic!("login ended without a device code");
    };
    let (running, (), ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(worker.login("fixture"), serve, cancel)
    })
    .await
    .expect("bounded login cancellation");
    assert!(running);
    let messages: Vec<_> = std::iter::from_fn(|| ui.try_recv().ok()).collect();
    assert!(messages.iter().any(|msg| matches!(msg, Msg::Busy(None))));
    assert!(
        messages
            .iter()
            .any(|msg| matches!(msg, Msg::System(text) if text.contains("sign-in cancelled")))
    );
    assert!(messages.iter().any(|msg| matches!(msg, Msg::Restored(inputs) if inputs.len() == 1 && inputs[0].text == "keep this draft")));
    assert!(matches!(worker.next().await, Some(Cmd::SetRounds(3))));
    assert!(
        tokio::time::timeout(Duration::from_millis(1500), listener.accept())
            .await
            .is_err(),
        "login kept polling after cancellation"
    );
    assert!(worker.followups.is_empty());
    assert!(
        crate::durable::log::read(worker.loop_.session_path())
            .expect("journal")
            .iter()
            .all(|line| !matches!(line, crate::durable::log::Line::User { .. }))
    );
    std::fs::remove_dir_all(workspace).expect("workspace");
}

#[tokio::test]
async fn cancelled_compaction_keeps_the_conversation_and_returns_pending_input() {
    let (mut worker, tx, mut ui, workspace) = worker_ui();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    worker.loop_.set_provider(
        Client::new("", format!("http://{address}/v1")).expect("provider"),
        "test",
    );
    worker.loop_ = worker.loop_.restore(vec![
        crate::provider::Message::user("earlier request"),
        crate::provider::AssistantMessage {
            content: Some("earlier answer".to_string()),
            ..Default::default()
        }
        .into_message(),
        crate::provider::Message::user("current request"),
    ]);
    let journal = std::fs::read(worker.loop_.session_path()).expect("original journal");
    let cancel = async {
        let (mut connection, _) = listener.accept().await.expect("summary request");
        let mut request = [0; 8192];
        let count = connection.read(&mut request).await.expect("request");
        assert!(
            String::from_utf8_lossy(&request[..count]).starts_with("POST /v1/chat/completions ")
        );
        tx.send(Cmd::Queue("keep this next prompt".into()))
            .expect("queue");
        tx.send(Cmd::SetRounds(3)).expect("setting");
        tx.send(Cmd::Cancel).expect("cancel");
        while connection.read(&mut request).await.expect("closed request") != 0 {}
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(worker.command("/compact"), cancel)
    })
    .await
    .expect("bounded compaction cancellation");
    assert_eq!(result, Some(true));
    assert_eq!(worker.loop_.history_len(), 3);
    assert_eq!(
        std::fs::read(worker.loop_.session_path()).expect("journal after cancellation"),
        journal
    );
    let messages: Vec<_> = std::iter::from_fn(|| ui.try_recv().ok()).collect();
    assert!(messages.iter().any(|msg| matches!(msg, Msg::Busy(None))));
    assert!(messages.iter().any(|msg| matches!(msg, Msg::System(text) if text == "Compaction cancelled. The conversation is unchanged.")));
    assert!(messages.iter().any(|msg| matches!(msg, Msg::Restored(inputs) if inputs.len() == 1 && inputs[0].text == "keep this next prompt")));
    assert!(matches!(worker.next().await, Some(Cmd::SetRounds(3))));
    assert!(worker.followups.is_empty());
    std::fs::remove_dir_all(workspace).expect("workspace");
}

#[tokio::test]
async fn fork_announces_the_active_session_and_keeps_its_parent() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    let parent = worker.loop_.session_id();
    assert_eq!(worker.command("/fork").await, Some(true));
    let child = worker.loop_.session_id();
    assert_ne!(child, parent);
    assert!(
        std::iter::from_fn(|| ui.try_recv().ok())
            .any(|msg| matches!(msg, Msg::Session(id) if id == child))
    );
    let lines = crate::durable::log::read(worker.loop_.session_path()).expect("forked session");
    assert!(lines.iter().any(|line| matches!(line, crate::durable::log::Line::Session { id, parent: Some(recorded), .. } if id.to_string() == child && recorded.to_string() == parent)));
    std::fs::remove_dir_all(workspace).expect("workspace");
}

#[tokio::test]
async fn earlier_branch_replays_the_selected_prefix_and_rejects_stale_sessions() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    let parent = worker.loop_.session_id();
    let path = worker.loop_.session_path().to_path_buf();
    let mut journal = crate::durable::log::Log::open(&path).expect("journal");
    for text in ["first saved prompt", "later saved prompt"] {
        journal
            .append(&crate::durable::log::Line::User {
                text: text.to_string(),
                source: None,
                images: Vec::new(),
                start: true,
            })
            .expect("saved prompt");
        journal
            .append(&crate::durable::log::Line::Close { reason: None })
            .expect("closed turn");
    }
    let bytes = std::fs::read(&path).expect("parent");
    assert!(
        worker
            .dispatch(Cmd::Branch {
                session: "stale session".to_string(),
                turn: 0
            })
            .await
    );
    assert_eq!(worker.loop_.session_id(), parent);
    let rejected: Vec<_> = std::iter::from_fn(|| ui.try_recv().ok()).collect();
    assert!(rejected.iter().any(
        |message| matches!(message, Msg::Failed(text) if text.contains("active session changed"))
    ));
    assert!(
        worker
            .dispatch(Cmd::Branch {
                session: parent.clone(),
                turn: 0
            })
            .await
    );
    let messages: Vec<_> = std::iter::from_fn(|| ui.try_recv().ok()).collect();
    assert!(matches!(messages.first(), Some(Msg::Reset)));
    assert!(matches!(messages.last(), Some(Msg::Busy(None))));
    let prompts: Vec<_> = messages
        .iter()
        .filter_map(|message| match message {
            Msg::User(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(prompts, ["first saved prompt"]);
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Msg::Session(id) if id == &worker.loop_.session_id()))
    );
    assert_ne!(worker.loop_.session_id(), parent);
    assert_eq!(std::fs::read(path).expect("parent after branch"), bytes);
    std::fs::remove_dir_all(workspace).expect("workspace");
}

#[tokio::test]
async fn failed_turn_returns_a_followup_instead_of_sending_it() {
    let (mut worker, tx, mut ui, workspace) = worker_ui();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider listener");
    let address = listener.local_addr().expect("provider address");
    worker.loop_.set_provider(
        Client::new("", format!("http://{address}/v1")).expect("provider"),
        "test",
    );
    let response = async {
        let (mut connection, _) = listener.accept().await.expect("active request");
        let mut bytes = [0_u8; 4096];
        assert!(connection.read(&mut bytes).await.expect("request") > 0);
        tx.send(Cmd::Queue("keep this followup".into()))
            .expect("followup");
        tx.send(Cmd::SetRounds(24)).expect("next command");
        connection.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 28\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"bad\"}}\n").await.expect("failed response");
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        let (_, running) = tokio::join!(response, worker.turn("running".into()));
        assert!(running);
    })
    .await
    .expect("bounded failure");
    let pending = std::iter::from_fn(|| ui.try_recv().ok())
        .find_map(|msg| match msg {
            Msg::Restored(inputs) => Some(inputs),
            _ => None,
        })
        .expect("restored input");
    assert_eq!(pending, vec![UserInput::from("keep this followup")]);
    assert!(matches!(worker.next().await, Some(Cmd::SetRounds(24))));
    std::fs::remove_dir_all(workspace).expect("scratch workspace");
}

#[tokio::test]
async fn retry_preserves_the_original_request_images_and_completed_tool_results() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    assert_eq!(worker.command("/retry").await, Some(true));
    assert_eq!(
        system_line(&mut ui).await,
        "No failed or cancelled turn to retry."
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    worker.loop_.set_provider(
        Client::new("", format!("http://{address}/v1")).expect("provider"),
        "test",
    );
    worker.loop_.set_verify(None);
    let responses = [
        (
            "200 OK",
            serde_json::json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"write-once","type":"function","function":{"name":"write_file","arguments":"{\"path\":\"once.txt\",\"content\":\"first version\"}"}}]},"finish_reason":"tool_calls"}]}),
        ),
        (
            "400 Bad Request",
            serde_json::json!({"error":{"message":"fixture failure"}}),
        ),
        (
            "200 OK",
            serde_json::json!({"choices":[{"message":{"content":"Recovered without repeating the write."},"finish_reason":"stop"}]}),
        ),
    ];
    let serve = async {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut connection, _) = listener.accept().await.expect("request");
            let mut bytes = Vec::new();
            let (offset, length) = loop {
                let mut chunk = [0; 4096];
                let read = connection.read(&mut chunk).await.expect("headers");
                assert!(read > 0);
                bytes.extend_from_slice(&chunk[..read]);
                if let Some(end) = bytes.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().expect("length"))
                        })
                        .expect("body length");
                    break (end + 4, length);
                }
            };
            while bytes.len() < offset + length {
                let mut chunk = [0; 4096];
                let read = connection.read(&mut chunk).await.expect("body");
                assert!(read > 0);
                bytes.extend_from_slice(&chunk[..read]);
            }
            requests.push(
                serde_json::from_slice::<serde_json::Value>(&bytes[offset..offset + length])
                    .expect("JSON request"),
            );
            let body = body.to_string();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            connection
                .write_all(response.as_bytes())
                .await
                .expect("response");
        }
        requests
    };
    let path = workspace.join("pasted.png");
    image::RgbImage::from_pixel(2, 2, image::Rgb([255, 0, 0]))
        .save(&path)
        .expect("image fixture");
    let image =
        crate::provider::Image::encode("image/png", &std::fs::read(&path).expect("image bytes"));
    std::fs::write(workspace.join("reference.txt"), "Use a single write.").expect("reference");
    let original = "write the file once\n  @reference.txt";
    let mut input = crate::tools::attach(&workspace, original).expect("attachment");
    input.images.push(image.clone());
    let work = async {
        assert!(worker.turn(input).await);
        assert!(worker.last_turn.is_none(), "the provider failed");
        assert_eq!(
            std::fs::read_to_string(workspace.join("once.txt")).expect("first write"),
            "first version"
        );
        std::fs::write(workspace.join("once.txt"), "user amendment").expect("manual edit");
        assert_eq!(worker.command("/retry").await, Some(true));
        assert_eq!(
            std::fs::read_to_string(workspace.join("once.txt")).expect("preserved edit"),
            "user amendment"
        );
        assert_eq!(
            worker.last_turn.as_ref().expect("successful retry").text,
            "Recovered without repeating the write."
        );
        assert_eq!(worker.command("/retry").await, Some(true));
        assert!(std::iter::from_fn(|| ui.try_recv().ok()).any(|msg| matches!(msg, Msg::System(text) if text == "No failed or cancelled turn to retry.")));
    };
    let (requests, ()) =
        tokio::time::timeout(Duration::from_secs(10), async { tokio::join!(serve, work) })
            .await
            .expect("bounded retry");
    assert_eq!(requests.len(), 3);
    let messages = requests[2]["messages"].as_array().expect("retry messages");
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.to_string().contains("write the file once"))
            .count(),
        1
    );
    assert!(messages.iter().any(|message| {
        message
            .to_string()
            .contains(&base64::engine::general_purpose::STANDARD.encode(&image.bytes))
    }));
    assert!(messages.iter().any(|message| {
        message["role"] == "tool"
            && message["tool_call_id"] == "write-once"
            && message["content"]
                .as_str()
                .is_some_and(|text| text.contains("wrote once.txt"))
    }));
    worker.replay();
    let replayed: Vec<_> = std::iter::from_fn(|| ui.try_recv().ok()).collect();
    assert_eq!(
        replayed
            .iter()
            .filter_map(|msg| match msg {
                Msg::Image(image) => Some(image),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [&image]
    );
    let prompts: Vec<_> = replayed
        .into_iter()
        .filter_map(|msg| match msg {
            Msg::User(text) => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(prompts[0], original);
    assert!(
        messages
            .iter()
            .any(|message| message.to_string().contains("Use a single write."))
    );
    let exported = crate::session::export(worker.loop_.session_path()).expect("export");
    assert!(exported.contains(original));
    assert!(!exported.contains("Use a single write."));
    std::fs::remove_dir_all(workspace).expect("workspace");
}

#[tokio::test]
async fn cancellation_returns_pending_prompts_without_starting_a_followup() {
    let (mut worker, tx, mut ui, workspace) = worker_ui();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider listener");
    let address = listener.local_addr().expect("provider address");
    worker.loop_.set_provider(
        Client::new("", format!("http://{address}/v1")).expect("provider"),
        "test",
    );
    let image = b"\x89PNG\r\n\x1a\n";
    std::fs::write(workspace.join("reference.png"), image).expect("file image");
    let mut first = crate::tools::attach(&workspace, "first @reference.png").expect("attach");
    first
        .images
        .push(crate::provider::Image::encode("image/png", b"pasted image"));
    let (finished, completion) = tokio::sync::oneshot::channel();
    let submit = async {
        let (_connection, _) = listener.accept().await.expect("active provider request");
        tx.send(Cmd::Steer("steer next".into())).expect("steer");
        tx.send(Cmd::Queue(first)).expect("first followup");
        tx.send(Cmd::Queue("second prompt".into()))
            .expect("second followup");
        tx.send(Cmd::Cancel).expect("cancel");
        tx.send(Cmd::SetRounds(24)).expect("next command");
        completion.await.expect("turn finished");
    };
    let turn = async {
        assert!(worker.turn("running".into()).await);
        finished.send(()).expect("release provider connection");
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(submit, turn);
    })
    .await
    .expect("bounded cancellation");
    let pending = std::iter::from_fn(|| ui.try_recv().ok())
        .find_map(|msg| match msg {
            Msg::Restored(inputs) => Some(inputs),
            _ => None,
        })
        .expect("restored prompts");
    assert_eq!(
        pending
            .iter()
            .map(|input| input.text.as_str())
            .collect::<Vec<_>>(),
        ["steer next", "first @reference.png", "second prompt"]
    );
    assert_eq!(
        pending[1].images,
        [crate::provider::Image::encode("image/png", b"pasted image")]
    );
    assert!(matches!(worker.next().await, Some(Cmd::SetRounds(24))));
    assert!(worker.steering.lock().expect("steering").is_empty());
    std::fs::remove_dir_all(workspace).expect("scratch workspace");
}

#[tokio::test]
async fn followup_runs_without_consuming_the_next_command() {
    let (mut worker, tx, workspace) = worker();
    worker.followups.push_back("queued".into());
    tx.send(Cmd::SetRounds(24))
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(matches!(worker.next().await, Some(Cmd::Submit(input)) if input.text == "queued"));
    assert!(matches!(worker.next().await, Some(Cmd::SetRounds(24))));
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn deferred_setting_runs_before_a_queued_turn() {
    let (mut worker, _tx, workspace) = worker();
    worker.followups.push_back("queued".into());
    worker.deferred.push_back(Cmd::SetVerify(true));

    assert!(matches!(worker.next().await, Some(Cmd::SetVerify(true))));
    assert!(matches!(worker.next().await, Some(Cmd::Submit(input)) if input.text == "queued"));
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn auto_reply_only_replaces_a_tick() {
    let (mut worker, tx, workspace) = worker();
    worker.pending_mail = true;
    worker.last_auto = Instant::now() - Duration::from_secs(11);
    tx.send(Cmd::SetMode(true))
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(matches!(worker.next().await, Some(Cmd::SetMode(true))));
    tx.send(Cmd::Tick).unwrap_or_else(|error| panic!("{error}"));
    assert!(matches!(worker.next().await, Some(Cmd::Mail)));
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn endpoint_change_keeps_its_detected_context() {
    use crate::endpoint::{Endpoint, Wire};

    let (mut worker, _tx, workspace) = worker();
    let endpoint = Endpoint {
        name: "local".to_string(),
        model: "qwen-next".to_string(),
        base_url: Some("http://localhost/v1".to_string()),
        api_key: Some("key".to_string()),
        source: None,
        oauth: None,
        body: None,
        context_window: Some(65_536),
        idle_timeout_secs: None,
        effort: None,
        price: None,
        env: Default::default(),
        headers: Default::default(),
        wire: Wire::Completions,
    };

    worker.drive(Ok(endpoint)).await;

    assert_eq!(worker.loop_.model(), "qwen-next");
    assert_eq!(worker.endpoint, "local");
    assert_eq!(worker.loop_.context_window(), Some(65_536));
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn the_card_seats_clears_and_reports_roles() {
    use crate::workflow::{Role, Seat};
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    worker.loop_.set_seat(
        Role::Plan,
        Client::new("", "http://localhost/v1").unwrap_or_else(|error| panic!("{error}")),
        "plan-model",
        None,
        None,
    );
    worker.workflow.seats.insert(
        Role::Plan,
        Seat {
            endpoint: "local".to_string(),
            model: Some("plan-model".to_string()),
        },
    );
    worker.send_workflow();
    let Some(Msg::Workflow { name, seats, .. }) = ui.recv().await else {
        panic!("expected a workflow message");
    };
    assert_eq!(name, None);
    assert_eq!(seats.len(), Role::ALL.len());
    assert_eq!(seats[1].as_deref(), Some("local\tplan-model"));
    assert_eq!(seats[2], None);

    worker.clear_role(Role::Plan);
    assert_eq!(worker.loop_.seat_model(Role::Plan), None);
    let Some(Msg::Workflow { seats, .. }) = ui.recv().await else {
        panic!("expected a workflow message");
    };
    assert_eq!(seats[1], None);
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

async fn system_line(ui: &mut UnboundedReceiver<Msg>) -> String {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), ui.recv()).await {
            Ok(Some(Msg::System(text))) => return text,
            Ok(Some(Msg::Failed(text))) => panic!("worker failed: {text}"),
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => panic!("no system line arrived"),
        }
    }
}

#[tokio::test]
async fn goal_clear_ends_the_active_goal() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    assert_eq!(worker.command("/goal clear").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "no active goal");
    worker
        .loop_
        .select_goal("finish it", &[], 3)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(worker.command("/goal clear").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "goal cleared — finish it");
    assert!(
        worker
            .loop_
            .active_goal()
            .unwrap_or_else(|error| panic!("{error}"))
            .is_none()
    );
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn steps_needs_a_readable_plan_and_clears_cleanly() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    assert_eq!(worker.command("/steps").await, Some(true));
    assert!(system_line(&mut ui).await.starts_with("/steps PLAN.md"));
    assert_eq!(worker.command("/steps clear").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "no active steps run");
    assert_eq!(worker.command("/steps missing.md").await, Some(true));
    let failed = loop {
        match tokio::time::timeout(Duration::from_secs(5), ui.recv()).await {
            Ok(Some(Msg::Failed(text))) => break text,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => panic!("no failure arrived"),
        }
    };
    assert!(failed.contains("cannot read"), "{failed}");
    worker
        .loop_
        .select_steps("## Steps\n\n1. One.\n2. Two.\n", &[])
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(worker.command("/steps clear").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "steps cleared");
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn effort_and_name_take_arguments_in_the_interface() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    assert_eq!(worker.command("/effort low").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "effort: low · not saved");
    assert_eq!(worker.command("/name widget spike").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "session named widget spike");
    assert_eq!(worker.loop_.name().as_deref(), Some("widget spike"));
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn bare_resume_reports_when_the_workspace_has_no_previous_session() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();

    assert_eq!(worker.command("/resume").await, Some(true));
    let failed = loop {
        match tokio::time::timeout(Duration::from_secs(5), ui.recv()).await {
            Ok(Some(Msg::Failed(text))) => break text,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => panic!("no failure arrived"),
        }
    };

    assert_eq!(
        failed,
        "configuration error: no previous session for this workspace"
    );
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn startup_replays_saved_conversation_and_tool_results() {
    use std::io::Write;
    let (worker, tx, mut ui, workspace) = worker_ui();
    let mut journal = std::fs::OpenOptions::new()
        .append(true)
        .open(worker.loop_.session_path())
        .expect("session journal");
    for line in [
        serde_json::json!({"kind":"user","text":"fix arithmetic\n\nHarness state: internal instructions","start":true}),
        serde_json::json!({"kind":"assistant","text":"Checking it.","tool_calls":[{"id":"read-1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"calculator.py\"}"}}],"usage":{"input":10,"output":3,"cached":2,"millis":40}}),
        serde_json::json!({"kind":"tool","call":"read-1","text":"file missing","success":false,"millis":1}),
        serde_json::json!({"kind":"assistant","text":"The file is missing.","usage":{"input":12,"output":5,"cached":4,"millis":50}}),
        serde_json::json!({"kind":"receipt","text":"The file is missing.\n\nTurn receipt\n- Changed: none"}),
        serde_json::json!({"kind":"close"}),
        serde_json::json!({"kind":"charge","usage":{"input":7,"output":2,"cached":1,"millis":20},"note":"review","model":"review-model"}),
        serde_json::json!({"kind":"charge","usage":{"input":8,"output":4,"cached":0,"millis":30},"note":"scout","model":"scout-model"}),
    ] {
        writeln!(journal, "{line}").expect("saved record");
    }
    drop(journal);
    let mut durable = crate::durable::log::State::default();
    for line in crate::durable::log::read(worker.loop_.session_path()).expect("journal") {
        durable.apply(line);
    }
    tx.send(Cmd::Quit).expect("stop after startup");
    worker.run().await;
    let mut messages = Vec::new();
    while let Ok(message) = ui.try_recv() {
        messages.push(message);
    }
    let restored: u64 = messages
        .iter()
        .filter_map(|message| match message {
            Msg::Event(crate::tools::Event::Usage {
                prompt_tokens,
                completion_tokens,
                ..
            }) => Some(u64::from(*prompt_tokens) + u64::from(*completion_tokens)),
            _ => None,
        })
        .sum();
    assert_eq!(restored, durable.spent);
    assert!(messages.iter().any(|message| matches!(message, Msg::System(text) if text.contains("Historical reasoning tokens and cost are unavailable"))));
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Msg::User(text) if text == "fix arithmetic"))
    );
    assert!(messages.iter().any(|message| matches!(message, Msg::Event(crate::tools::Event::ToolResult { success: false, output, .. }) if output == "file missing")));
    assert!(messages.iter().any(|message| matches!(
        message,
        Msg::Event(crate::tools::Event::Usage {
            prompt_tokens: 12,
            completion_tokens: 5,
            cached_tokens: 4,
            ..
        })
    )));
    assert!(messages.iter().any(|message| matches!(message, Msg::Receipt(text) if text == "The file is missing.\n\nTurn receipt\n- Changed: none")));
    std::fs::remove_dir_all(workspace).expect("scratch workspace");
}
