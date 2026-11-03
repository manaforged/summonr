use super::*;
use std::io::Write;

#[test]
fn a_named_worker_recovers_and_stops_through_the_executable_without_replacement() {
    let worker = Model::start(vec![
        Step::Fail(
            400,
            "0",
            r#"{"error":{"message":"initial worker request rejected","type":"invalid_request_error"}}"#,
        ),
        Step::Text("child recovered"),
    ]);
    let home = tempfile::tempdir().expect("isolated delegation home");
    std::fs::write(home.path().join("config.toml"), "[modules]\nmail = true\n")
        .expect("mail is explicitly enabled for the delegation scenario");
    let run = Journey::at(
        home,
        vec![
            Step::Call(
                "task",
                json!({
                    "access": "research",
                    "assignment": "read the assignment",
                    "background": true,
                    "effort": "low",
                    "endpoint": "worker",
                    "name": "boundary-worker",
                    "workspace": "shared"
                }),
            ),
            Step::Text("worker started"),
            Step::Text(r#"{"verdict":"done","reason":"the named worker is addressable"}"#),
            Step::Call(
                "send",
                json!({"to": "boundary-worker", "text": "try the assignment again"}),
            ),
            Step::Text("worker resumed"),
            Step::Text(r#"{"verdict":"done","reason":"the recovery message was delivered"}"#),
        ],
    );
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(run.home.path().join("config.toml"))
        .expect("the terminal fixture wrote its endpoint configuration");
    writeln!(
        config,
        "\n[endpoint.worker]\nbase_url = 'http://127.0.0.1:{}/v1'\nmodel = 'replay'\ncontext_window = 32768",
        worker.port
    )
    .expect("the worker endpoint is configured");
    drop(config);

    run.say("start the worker");
    run.terminal.wait("the parent response", |screen| {
        screen.contains("worker started")
    });
    run.say("recover the worker");
    run.terminal.wait("the recovery response", |screen| {
        screen.contains("worker resumed")
    });
    let deadline = Instant::now() + QUICK;
    loop {
        if run
            .log()
            .iter()
            .any(|line| line["kind"] == "assistant" && line["text"] == "child recovered")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the named worker never recovered; worker requests: {}; tool results: {:?}; outcomes: {:?}",
            worker.requests().len(),
            run.model.tool_results(),
            run.log()
                .iter()
                .filter(|line| matches!(
                    line["kind"].as_str(),
                    Some("assistant" | "close" | "child")
                ))
                .collect::<Vec<_>>()
        );
        support::pause(Duration::from_millis(20));
    }
    let child_ids: Vec<String> = run
        .log()
        .iter()
        .filter(|line| line["kind"] == "session" && line["depth"] == 1)
        .map(|line| line["id"].as_str().expect("a child has an id").to_string())
        .collect();
    assert_eq!(child_ids.len(), 1, "recovery must not spawn another worker");
    let requests = worker.requests();
    assert_eq!(requests.len(), 2, "one failed turn and one mailed recovery");
    assert!(
        requests
            .iter()
            .all(|body| body.get("reasoning_effort").is_none())
    );
    assert!(
        requests[1]["messages"]
            .to_string()
            .contains("try the assignment again")
    );

    run.say("/stop");
    let deadline = Instant::now() + QUICK;
    loop {
        let stopped = run.log().into_iter().find(|line| {
            line["kind"] == "child"
                && line["record"]["id"] == child_ids[0]
                && line["record"]["state"]["status"] != "running"
        });
        if let Some(stopped) = stopped {
            assert_eq!(stopped["record"]["name"], "boundary-worker");
            assert_eq!(
                stopped["record"]["state"]["result"]["diagnostic"],
                "stopped by user"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "stop did not settle the original worker"
        );
        support::pause(Duration::from_millis(20));
    }
}
