use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BLESS: &str = "SUMMONR_BLESS";
const OFFLINE_PROXY: &str = "http://127.0.0.1:9";
const OFFLINE_SECS: u64 = 3;

struct Home {
    dir: tempfile::TempDir,
    env: Vec<(&'static str, String)>,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".summonr")).unwrap();
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        Self {
            dir,
            env: Vec::new(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn config(&self, text: &str) -> &Self {
        std::fs::write(self.path().join(".summonr/config.toml"), text).unwrap();
        self
    }

    fn listings(&self, text: &str) -> &Self {
        std::fs::write(self.path().join(".summonr/models.json"), text).unwrap();
        self
    }

    fn key(mut self, name: &'static str) -> Self {
        self.env.push((name, "test-key".to_string()));
        self
    }

    fn offline(mut self) -> Self {
        for name in ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"] {
            self.env.push((name, OFFLINE_PROXY.to_string()));
        }
        self
    }

    fn list(&self, args: &[&str]) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
            .current_dir(self.path())
            .env_clear()
            .env("HOME", self.path())
            .env("USERPROFILE", self.path())
            .env("SUMMONR_HOME", self.path().join(".summonr"))
            .env("PATH", self.path().join("bin"))
            .envs(std::env::var_os("SYSTEMROOT").map(|root| ("SYSTEMROOT", root)))
            .envs(self.env.iter().map(|(name, value)| (*name, value)))
            .arg("endpoints")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

fn golden(name: &str, text: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/endpoints")
        .join(format!("{name}.txt"));
    if std::env::var_os(BLESS).is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        return;
    }
    let held = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing; run with {BLESS}=1", path.display()));
    assert_eq!(text, held, "{name} drifted; run with {BLESS}=1 to accept");
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn models_server() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

fn serve_once(listener: &TcpListener, body: &str) {
    let (mut stream, _) = listener.accept().unwrap();
    let mut request = [0; 4096];
    let _ = stream.read(&mut request).unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

#[test]
fn with_nothing_set_up_every_endpoint_says_how_to_set_it_up() {
    let home = Home::new();
    golden("unconfigured", &home.list(&[]));
}

#[test]
fn set_up_endpoints_list_first_with_their_key_source_and_hide_the_rest() {
    let home = Home::new().key("ANTHROPIC_API_KEY").key("DEEPSEEK_API_KEY");
    let listed = home.list(&[]);
    golden("configured", &listed);
    golden("configured-all", &home.list(&["--all"]));
    let offline = home.offline();
    let started = Instant::now();
    assert_eq!(offline.list(&[]), listed);
    assert!(started.elapsed() < Duration::from_secs(OFFLINE_SECS));
}

#[test]
fn a_plain_listing_never_fetches_the_models_catalog() {
    let (catalog, port) = models_server();
    catalog.set_nonblocking(true).unwrap();
    let mut home = Home::new().key("ANTHROPIC_API_KEY");
    home.env.push((
        "SUMMONR_MODELS_URL",
        format!("http://127.0.0.1:{port}/api.json"),
    ));
    home.list(&[]);
    home.list(&["--all"]);
    assert!(catalog.accept().is_err(), "summonr fetched the catalog");
}

#[test]
fn a_cached_listing_shows_its_model_count_and_reads_stale_when_old() {
    let home = Home::new().key("ANTHROPIC_API_KEY").key("DEEPSEEK_API_KEY");
    home.listings(&format!(
        r#"{{"anthropic-api":{{"models":[{{"id":"claude-opus-5-5"}},{{"id":"claude-sonnet-5-5"}}],"fetched":{}}},"deepseek":{{"models":[{{"id":"deepseek-flash"}}],"fetched":1}}}}"#,
        now()
    ));
    golden("cached", &home.offline().list(&[]));
}

#[test]
fn a_custom_server_without_a_listing_is_asked_once_then_read_from_the_cache() {
    let body = r#"{"data":[{"id":"m"},{"id":"n"}]}"#;
    let (listener, port) = models_server();
    let home = Home::new();
    home.config(&format!(
        "[endpoint.lab]\nbase_url = \"http://127.0.0.1:{port}/v1\"\nmodel = \"m\"\n"
    ));
    let server = std::thread::spawn(move || {
        serve_once(&listener, body);
        listener
    });
    let first = home.list(&[]).replace(&port.to_string(), "PORT");
    let listener = server.join().unwrap();
    golden("custom-server", &first);
    listener.set_nonblocking(true).unwrap();
    let second = home.list(&[]).replace(&port.to_string(), "PORT");
    assert!(
        listener.accept().is_err(),
        "a cached server was asked again"
    );
    assert_eq!(second, first);
}

#[cfg(unix)]
#[test]
fn listing_never_starts_claude_code() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new();
    home.config("models_catalog = false\n[provider.anthropic]\naccess = \"plan\"\n");
    let marker = home.path().join("claude-started");
    let shim = home.path().join("bin/claude");
    std::fs::write(
        &shim,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    let home = home.offline();
    let checked = home.list(&["--all", "--check"]);
    for args in [&[][..], &["--all"]] {
        home.list(args);
    }
    assert!(
        !marker.exists(),
        "summonr started Claude Code to list models"
    );
    assert!(
        checked
            .lines()
            .any(|line| line.contains(" claude ") && line.contains(" models")),
        "{checked}"
    );
}
