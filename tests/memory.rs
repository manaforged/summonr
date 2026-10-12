use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const REPLY: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"the walrus is noted\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

fn provider() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&bodies);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream);
            let mut length = 0;
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 2 {
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
                line.clear();
            }
            let mut body = vec![0_u8; length];
            drop(reader.read_exact(&mut body));
            seen.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&body).into_owned());
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{REPLY}",
                REPLY.len()
            );
            drop(reader.get_mut().write_all(reply.as_bytes()));
        }
    });
    (port, bodies)
}

fn summonr(home: &Path, work: &Path, port: u16, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(args)
        .current_dir(work)
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("SUMMONR_HOME", home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(std::env::var_os("SYSTEMROOT").map(|root| ("SYSTEMROOT", root)))
        .env(
            "SUMMONR_MODELS_URL",
            format!("http://127.0.0.1:{port}/registry.json"),
        )
        .stdin(Stdio::null())
        .output()
        .unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn repository() -> tempfile::TempDir {
    let work = tempfile::tempdir().unwrap();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(work.path())
        .status()
        .unwrap();
    assert!(status.success());
    work
}

fn project_memory(home: &Path, work: &Path) -> PathBuf {
    summonr::memory::files(home, work)
        .into_iter()
        .find(|file| file.scope == summonr::memory::Scope::Project)
        .unwrap()
        .path
}

fn configure(home: &Path, port: u16, extra: &str) {
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[endpoint.local]\nbase_url = \"http://127.0.0.1:{port}/v1\"\nmodel = \"m\"\napi_key = \"k\"\ncontext_window = 32768\n{extra}"
        ),
    )
    .unwrap();
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn sent(bodies: &Arc<Mutex<Vec<String>>>, text: &str) -> bool {
    bodies
        .lock()
        .unwrap()
        .iter()
        .any(|body| body.contains(text))
}

#[test]
fn saved_memory_reaches_the_model_in_its_own_repository_only() {
    let home = tempfile::tempdir().unwrap();
    let work = repository();
    let other = repository();
    write(
        &home.path().join("memory/MEMORY.md"),
        "2026-10-11 the user prefers tabs\n",
    );
    write(
        &project_memory(home.path(), work.path()),
        "2026-10-11 the api tests need the redis fixture\n",
    );
    let (port, bodies) = provider();
    configure(home.path(), port, "");
    let text = summonr(home.path(), work.path(), port, &["-e", "local", "-p", "hi"]);
    assert!(sent(&bodies, "the user prefers tabs"), "{text}");
    assert!(
        sent(&bodies, "the api tests need the redis fixture"),
        "{text}"
    );

    let (port, bodies) = provider();
    configure(home.path(), port, "");
    let text = summonr(
        home.path(),
        other.path(),
        port,
        &["-e", "local", "-p", "hi"],
    );
    assert!(sent(&bodies, "the user prefers tabs"), "{text}");
    assert!(!sent(&bodies, "redis fixture"), "{text}");
}

#[test]
fn memory_stays_out_when_the_module_is_off_or_the_workspace_untrusted() {
    let home = tempfile::tempdir().unwrap();
    let work = repository();
    write(
        &home.path().join("memory/MEMORY.md"),
        "2026-10-11 the user prefers tabs\n",
    );
    let (port, bodies) = provider();
    configure(home.path(), port, "[modules]\nmemory = false\n");
    let text = summonr(home.path(), work.path(), port, &["-e", "local", "-p", "hi"]);
    assert!(!bodies.lock().unwrap().is_empty(), "{text}");
    assert!(!sent(&bodies, "prefers tabs"), "{text}");

    let (port, bodies) = provider();
    configure(home.path(), port, "");
    let text = summonr(
        home.path(),
        work.path(),
        port,
        &["-e", "local", "--no-trust", "-p", "hi"],
    );
    assert!(!bodies.lock().unwrap().is_empty(), "{text}");
    assert!(!sent(&bodies, "prefers tabs"), "{text}");
}

#[test]
fn sessions_grep_finds_text_from_a_past_session() {
    let home = tempfile::tempdir().unwrap();
    let work = repository();
    let (port, _bodies) = provider();
    configure(home.path(), port, "");
    let text = summonr(
        home.path(),
        work.path(),
        port,
        &["-e", "local", "-p", "remember the walrus"],
    );
    assert!(text.contains("walrus"), "{text}");
    let found = summonr(
        home.path(),
        work.path(),
        port,
        &["sessions", "--grep", "WALRUS"],
    );
    assert!(found.contains("the walrus is noted"), "{found}");
    let date = found.split('\t').nth(1).unwrap_or_default();
    assert!(date.starts_with("20") && date.len() == 16, "{found}");
    let none = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["sessions", "--grep", "narwhal"])
        .current_dir(work.path())
        .env("HOME", home.path())
        .env("SUMMONR_HOME", home.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(none.status.code(), Some(1));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&none.stdout),
        String::from_utf8_lossy(&none.stderr)
    );
    assert_eq!(text.trim(), "no matches");
    let alone = summonr(home.path(), work.path(), port, &["--grep", "walrus"]);
    assert!(
        alone.contains("--grep works with `summonr sessions`"),
        "{alone}"
    );
}

#[test]
fn a_mistyped_command_is_not_sent_to_the_model() {
    let home = tempfile::tempdir().unwrap();
    let work = repository();
    let (port, bodies) = provider();
    configure(home.path(), port, "");
    let text = summonr(home.path(), work.path(), port, &["sesions"]);
    assert!(text.contains("did you mean `sessions`"), "{text}");
    assert!(bodies.lock().unwrap().is_empty(), "{text}");
}
