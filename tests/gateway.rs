use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn summonr(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_summonr"));
    command
        .current_dir(home)
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("SUMMONR_HOME", home.join(".summonr"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(std::env::var_os("SYSTEMROOT").map(|root| ("SYSTEMROOT", root)))
        .stdin(Stdio::null());
    command
}

fn quiet<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut args = args.to_vec();
    if args.first() == Some(&"add") && !args.contains(&"--notify") {
        args.extend(["--notify", "never"]);
    }
    args
}

fn cron(home: &Path, args: &[&str]) -> String {
    let output = summonr(home)
        .arg("cron")
        .args(quiet(args))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn serve(home: &Path) -> (Child, u16) {
    let mut child = summonr(home)
        .args(["gateway", "webhook", "--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port = line.trim().rsplit(':').next().unwrap().parse().unwrap();
    (child, port)
}

fn bodies(workspace: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(workspace.join(".summonr/jobs"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "body"))
                .collect()
        })
        .unwrap_or_default()
}

fn raw(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply
}

fn post(port: u16, method: &str, path: &str, token: Option<&str>, body: &str) -> String {
    let auth = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    raw(
        port,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn job(home: &Path, name: &str, schedule: &[&str], prompt: &str) -> String {
    let mut args = vec!["add", name];
    args.extend(schedule);
    args.extend(["-p", prompt]);
    cron(home, &args);
    cron(home, &["token", name])
        .lines()
        .next()
        .unwrap()
        .to_string()
}

fn stop(mut server: Child) {
    drop(server.kill());
    drop(server.wait());
}

#[test]
fn a_webhook_fires_its_cron_job_once_with_the_token() {
    let home = tempfile::tempdir().unwrap();
    let token = job(home.path(), "hook", &["--every", "1d"], "triage the event");
    let (server, port) = serve(home.path());

    assert!(post(port, "POST", "/run/hook", None, "{}").starts_with("HTTP/1.1 401"));
    assert!(post(port, "POST", "/run/hook", Some("wrong"), "{}").starts_with("HTTP/1.1 401"));
    assert!(post(port, "POST", "/run/nope", Some(&token), "{}").starts_with("HTTP/1.1 401"));
    let get = post(port, "GET", "/run/hook", Some(&token), "");
    assert!(get.starts_with("HTTP/1.1 405"), "{get}");
    assert!(get.contains("Allow: POST"), "{get}");
    let fired = post(port, "POST", "/run/hook", Some(&token), "{\"issue\":42}");
    assert!(fired.starts_with("HTTP/1.1 202"), "{fired}");
    assert!(fired.contains("\"job\":\"hook\""), "{fired}");
    let again = post(port, "POST", "/run/hook", Some(&token), "{}");
    assert!(again.starts_with("HTTP/1.1 429"), "{again}");
    assert!(again.contains("Retry-After:"), "{again}");
    assert!(again.contains("{\"error\""), "{again}");
    let chunked = raw(
        port,
        &format!(
            "POST /run/hook HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n0\r\n\r\n"
        ),
    );
    assert!(chunked.starts_with("HTTP/1.1 411"), "{chunked}");
    stop(server);

    let saved = bodies(home.path());
    assert_eq!(saved.len(), 1);
    assert_eq!(
        std::fs::read_to_string(&saved[0]).unwrap(),
        "{\"issue\":42}"
    );

    let started = Instant::now();
    let mut log = cron(home.path(), &["log", "hook"]);
    while log.contains("running") && started.elapsed() < Duration::from_secs(30) {
        std::thread::park_timeout(Duration::from_millis(200));
        log = cron(home.path(), &["log", "hook"]);
    }
    assert!(log.contains("error"), "{log}");
}

#[test]
fn removing_a_job_forgets_its_webhook_token() {
    let home = tempfile::tempdir().unwrap();
    let token = job(home.path(), "gone", &["--every", "1d"], "hi");
    cron(home.path(), &["remove", "gone"]);
    cron(home.path(), &["add", "gone", "--every", "1d", "-p", "hi"]);
    let (server, port) = serve(home.path());
    let reply = post(port, "POST", "/run/gone", Some(&token), "");
    stop(server);
    assert!(reply.starts_with("HTTP/1.1 401"), "{reply}");
}

#[test]
fn a_query_string_still_reaches_the_job_and_removal_deletes_payloads() {
    let home = tempfile::tempdir().unwrap();
    let token = job(home.path(), "q", &["--every", "1d"], "hi");
    let (server, port) = serve(home.path());
    let fired = post(port, "POST", "/run/q?source=ci", Some(&token), "{}");
    stop(server);
    assert!(fired.starts_with("HTTP/1.1 202"), "{fired}");
    assert_eq!(bodies(home.path()).len(), 1);
    cron(home.path(), &["remove", "q"]);
    assert!(bodies(home.path()).is_empty());
}

#[cfg(unix)]
#[test]
fn the_webhook_stops_on_sigterm_and_a_slow_client_blocks_nobody() {
    let home = tempfile::tempdir().unwrap();
    let token = job(home.path(), "s", &["--manual"], "hi");
    let (mut server, port) = serve(home.path());
    let stalled: Vec<TcpStream> = (0..12)
        .map(|_| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.write_all(b"POST /run/s HTTP/1.1\r\n").unwrap();
            stream
        })
        .collect();
    let started = Instant::now();
    let fired = post(port, "POST", "/run/s", Some(&token), "{}");
    assert!(fired.starts_with("HTTP/1.1 202"), "{fired}");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    let status = Command::new("kill")
        .args(["-TERM", &server.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let started = Instant::now();
    let code = loop {
        if let Some(status) = server.try_wait().unwrap() {
            break status.code();
        }
        assert!(started.elapsed() < Duration::from_secs(5), "still running");
        std::thread::park_timeout(Duration::from_millis(50));
    };
    assert_eq!(code, Some(143));
    drop(stalled);
}

#[test]
fn every_gateway_command_answers_help() {
    let home = tempfile::tempdir().unwrap();
    for words in [
        &["pair", "--help"][..],
        &["setup", "telegram", "--help"],
        &["webhook", "-h"],
    ] {
        let output = summonr(home.path())
            .arg("gateway")
            .args(words)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{words:?}");
        assert!(text.starts_with("usage:"), "{words:?}: {text}");
    }
}

#[test]
fn an_oversized_body_gets_a_readable_413_and_bad_tokens_are_explained() {
    let home = tempfile::tempdir().unwrap();
    let token = job(home.path(), "big", &["--manual"], "hi");
    let (server, port) = serve(home.path());
    let body = "x".repeat(70_000);
    let reply = post(port, "POST", "/run/big", Some(&token), &body);
    stop(server);
    assert!(reply.starts_with("HTTP/1.1 413"), "{reply}");
    assert!(reply.contains("{\"error\""), "{reply}");
    let mut setup = summonr(home.path())
        .args(["gateway", "setup", "telegram"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    setup
        .stdin
        .take()
        .unwrap()
        .write_all(b"not a token\n")
        .unwrap();
    let output = setup.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(
        text.contains("does not look like a Telegram bot token"),
        "{text}"
    );
    let output = summonr(home.path())
        .args(["gateway", "unpair", "abc"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("USER_ID is a number"), "{text}");
}
