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

fn cron(home: &Path, args: &[&str]) -> String {
    let output = summonr(home).arg("cron").args(args).output().unwrap();
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

fn post(port: u16, method: &str, path: &str, token: Option<&str>, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let auth = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply
}

#[test]
fn a_webhook_fires_its_cron_job_once_with_the_token() {
    let home = tempfile::tempdir().unwrap();
    cron(
        home.path(),
        &["add", "hook", "--every", "1d", "-p", "triage the event"],
    );
    let token = cron(home.path(), &["token", "hook"])
        .lines()
        .next()
        .unwrap()
        .to_string();
    let (mut server, port) = serve(home.path());

    assert!(post(port, "POST", "/run/hook", None, "{}").starts_with("HTTP/1.1 401"));
    assert!(post(port, "POST", "/run/hook", Some("wrong"), "{}").starts_with("HTTP/1.1 401"));
    assert!(post(port, "POST", "/run/nope", Some(&token), "{}").starts_with("HTTP/1.1 401"));
    assert!(post(port, "GET", "/run/hook", Some(&token), "").starts_with("HTTP/1.1 404"));
    let fired = post(port, "POST", "/run/hook", Some(&token), "{\"issue\":42}");
    assert!(fired.starts_with("HTTP/1.1 202"), "{fired}");
    assert!(fired.contains("\"job\":\"hook\""), "{fired}");
    let again = post(port, "POST", "/run/hook", Some(&token), "{}");
    assert!(again.starts_with("HTTP/1.1 429"), "{again}");
    drop(server.kill());
    drop(server.wait());

    let payloads = home.path().join(".summonr/cron/payloads");
    let saved: Vec<_> = std::fs::read_dir(&payloads).unwrap().flatten().collect();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        std::fs::read_to_string(saved[0].path()).unwrap(),
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
    cron(home.path(), &["add", "gone", "--every", "1d", "-p", "hi"]);
    let token = cron(home.path(), &["token", "gone"])
        .lines()
        .next()
        .unwrap()
        .to_string();
    cron(home.path(), &["remove", "gone"]);
    cron(home.path(), &["add", "gone", "--every", "1d", "-p", "hi"]);
    let (mut server, port) = serve(home.path());
    let reply = post(port, "POST", "/run/gone", Some(&token), "");
    drop(server.kill());
    drop(server.wait());
    assert!(reply.starts_with("HTTP/1.1 401"), "{reply}");
}
