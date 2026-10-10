use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[endpoint.local]\nbase_url = \"http://127.0.0.1:{port}/v1\"\nmodel = \"m\"\napi_key = \"k\"\ncontext_window = 32768\n"
        ),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["-e", "local", "-p", "--json", "hi"])
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
