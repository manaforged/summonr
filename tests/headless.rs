use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
fn pause(wait: Duration) {
    let until = Instant::now() + wait;
    while let Some(left) = until
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
    {
        std::thread::park_timeout(left);
    }
}

fn provider(reply: Option<&'static str>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for mut stream in listener.incoming().flatten() {
            let mut buffer = [0_u8; 65536];
            drop(stream.read(&mut buffer));
            match reply {
                Some(reply) => drop(stream.write_all(reply.as_bytes())),
                None => held.push(stream),
            }
        }
    });
    port
}

fn start(home: &Path, port: u16) -> Child {
    launch(home, port, &["-p", "--json", "hi"])
}

fn launch(home: &Path, port: u16, args: &[&str]) -> Child {
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[endpoint.local]\nbase_url = \"http://127.0.0.1:{port}/v1\"\nmodel = \"m\"\napi_key = \"k\"\ncontext_window = 32768\n"
        ),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["-e", "local"])
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("SUMMONR_HOME", home)
        .env(
            "SUMMONR_MODELS_URL",
            format!("http://127.0.0.1:{port}/registry.json"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn finish(child: Child) -> (Option<i32>, Vec<serde_json::Value>, String) {
    let output = child.wait_with_output().unwrap();
    let events = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.code(), events, stderr)
}

fn aborted(events: &[serde_json::Value]) -> Option<&str> {
    events
        .iter()
        .rfind(|event| event["type"] == "aborted")
        .and_then(|event| event["code"].as_str())
}

#[test]
fn a_rejected_key_ends_the_json_stream_with_an_error_event() {
    let home = tempfile::tempdir().unwrap();
    let port = provider(Some(
        "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: 41\r\nconnection: close\r\n\r\n{\"error\":{\"message\":\"invalid api key\"}}\n",
    ));
    let (code, events, stderr) = finish(start(home.path(), port));
    assert_eq!(code, Some(1), "{stderr}");
    assert_eq!(aborted(&events), Some("error"), "{events:?}\n{stderr}");
}

#[test]
fn a_broken_config_ends_the_json_stream_with_an_error_event() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), "this is = = not toml\n").unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["-p", "--json", "hi"])
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("SUMMONR_HOME", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (code, events, stderr) = finish(child);
    assert_eq!(code, Some(1), "{stderr}");
    assert_eq!(events.len(), 1, "one event: {events:?}\n{stderr}");
    assert_eq!(aborted(&events), Some("error"), "{events:?}\n{stderr}");
    assert!(
        events[0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("config.toml")),
        "{events:?}"
    );
}

#[cfg(unix)]
#[test]
fn an_interrupted_json_run_says_so_and_leaves_an_exit_record() {
    let home = tempfile::tempdir().unwrap();
    let port = provider(None);
    let child = start(home.path(), port);
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        let logs = std::fs::read_dir(home.path().join("logs")).map_or(0, Iterator::count);
        if logs > 0 {
            break;
        }
        pause(Duration::from_millis(50));
    }
    pause(Duration::from_millis(500));
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let (code, events, stderr) = finish(child);
    assert_eq!(code, Some(143), "{stderr}");
    assert_eq!(
        aborted(&events),
        Some("interrupted"),
        "{events:?}\n{stderr}"
    );
    let (_, _, next) = finish(start(
        home.path(),
        provider(Some(
            "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        )),
    ));
    assert!(!next.contains("without an exit record"), "{next}");
}

fn request_head(stream: &std::net::TcpStream) -> String {
    use std::io::BufRead;
    let Ok(clone) = stream.try_clone() else {
        return String::new();
    };
    let mut reader = std::io::BufReader::new(clone);
    let (mut head, mut length) = (String::new(), 0);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
        head.push_str(&line);
    }
    let mut body = vec![0; length];
    drop(reader.read_exact(&mut body));
    head
}

fn streaming(chunks: &'static [&'static str]) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let head = request_head(&stream);
            let path = head.split_whitespace().nth(1).unwrap_or("");
            let (kind, body) = if path.ends_with("/chat/completions") {
                let events: String = chunks
                    .iter()
                    .map(|chunk| format!("data: {chunk}\n\n"))
                    .chain(std::iter::once("data: [DONE]\n\n".to_owned()))
                    .collect();
                ("text/event-stream", events)
            } else {
                ("application/json", "{}".to_owned())
            };
            drop(write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            ));
        }
    });
    port
}

#[test]
fn a_text_run_keeps_stdout_to_the_answer_and_marks_reasoning_only_before_it() {
    let home = tempfile::tempdir().unwrap();
    let port = streaming(&[
        r#"{"choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"plan"}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"pytest runs "}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"reasoning_content":"more"}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"tests"},"finish_reason":"stop"}]}"#,
    ]);
    let output = launch(home.path(), port, &["-p", "hi"])
        .wait_with_output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert_eq!(stdout.trim(), "pytest runs tests", "{stderr}");
    assert_eq!(stderr.matches("… reasoning").count(), 1, "{stderr}");
    assert!(stderr.contains("Turn receipt"), "{stderr}");
}

#[test]
fn a_plain_run_names_its_workspace_check_and_a_json_run_does_not() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".summonr")).unwrap();
    std::fs::write(
        home.path().join(".summonr/check.toml"),
        "check = \"true\"\n",
    )
    .unwrap();
    let rejected = "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: 41\r\nconnection: close\r\n\r\n{\"error\":{\"message\":\"invalid api key\"}}\n";
    let (_, _, plain) = finish(launch(home.path(), provider(Some(rejected)), &["-p", "hi"]));
    assert!(plain.contains("summonr: workspace check: true"), "{plain}");
    let (_, _, json) = finish(start(home.path(), provider(Some(rejected))));
    assert!(!json.contains("workspace check"), "{json}");
}
