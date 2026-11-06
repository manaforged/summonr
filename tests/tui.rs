#![cfg(unix)]

use std::io::{Read, Write};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

#[path = "support/terminal.rs"]
mod terminal;

use terminal::Terminal;

const SIZES: [(u16, u16); 3] = [(100, 30), (120, 40), (223, 67)];
const DRAFT: &str = "breath of fire pixel style breath of fire pixel style breath of fire pixel style breath of fire pixel style breath of fire pixel style breath of fire pixel style tail";

trait Settle {
    fn settle(&self, what: &str, ready: impl Fn(&Splash) -> bool) -> Splash;
}

impl Settle for Terminal {
    fn settle(&self, what: &str, ready: impl Fn(&Splash) -> bool) -> Splash {
        let steady = Duration::from_millis(300);
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut last = None;
        let mut since = Instant::now();
        loop {
            let rows = self.rows();
            let splash = Splash::read(&rows).filter(&ready);
            let place = splash.as_ref().map(Splash::place);
            if place.is_none() || place != last {
                since = Instant::now();
            }
            let held = since.elapsed();
            if held >= steady
                && let Some(splash) = splash
            {
                return splash;
            }
            assert!(
                Instant::now() < deadline,
                "{what} never showed:\n{}",
                rows.join("\n")
            );
            last = place;
            let wait = self.wait_output(steady.saturating_sub(held));
            assert_ne!(
                wait,
                Err(RecvTimeoutError::Disconnected),
                "summonr exited before {what} showed:\n{}",
                rows.join("\n")
            );
        }
    }
}

#[derive(Debug)]
struct Splash {
    rows: Vec<String>,
    prompt: usize,
    hint: Option<usize>,
}

impl Splash {
    fn read(rows: &[String]) -> Option<Self> {
        Some(Self {
            rows: rows.to_vec(),
            prompt: rows.iter().rposition(|row| row.contains('❯'))?,
            hint: rows.iter().position(|row| row.contains("ask anything")),
        })
    }

    fn place(&self) -> [Option<usize>; 2] {
        [Some(self.prompt), self.hint]
    }

    fn composer_inside(&self) -> bool {
        self.hint.is_some_and(|row| row < self.prompt) && self.prompt < self.rows.len()
    }

    fn shows(&self, text: &str) -> bool {
        self.rows.iter().any(|row| row.contains(text))
    }

    fn screen(&self) -> String {
        self.rows.join("\n")
    }
}

#[test]
fn claude_skills_appear_in_slash_typeahead_and_refresh_without_a_restart() {
    let home = tempfile::tempdir().expect("isolated skill home");
    let installed = home.path().join(".claude/skills/probe-discovery");
    std::fs::create_dir_all(&installed).expect("Claude skill directory");
    std::fs::write(
        installed.join("SKILL.md"),
        "---\ndescription: inspect this project\n---\nInspect the project.\n",
    )
    .expect("Claude skill file");
    let terminal = Terminal::open(home.path(), 120, 40, 9);
    terminal.settle("the splash", |splash| splash.hint.is_some());
    let query = b"/probe-disc";
    terminal.send(query);
    terminal.settle("the Claude skill suggestion", |splash| {
        splash.shows("/probe-discovery")
    });
    terminal.send(&vec![0x7f; query.len()]);
    let added = home.path().join(".summonr/skills/probe-hot-added");
    std::fs::create_dir_all(&added).expect("new native skill directory");
    std::fs::write(
        added.join("SKILL.md"),
        "---\ndescription: added during this session\n---\nUse the current project.\n",
    )
    .expect("new native skill file");
    terminal.send(b"/probe-hot");
    terminal.settle("the newly installed skill suggestion", |splash| {
        splash.shows("/probe-hot-added")
    });
    terminal.send(&vec![0x7f; b"/probe-hot".len()]);
    terminal.send(b"/skills\r");
    terminal.settle("the installed skills listing", |splash| {
        splash.shows("added during this session")
    });
}

#[test]
fn the_splash_prompt_holds_its_row_inside_the_window_while_typing() {
    for (cols, rows) in SIZES {
        let home = tempfile::tempdir().unwrap();
        let terminal = Terminal::open(home.path(), cols, rows, 9);
        let idle = terminal.settle("the splash", |splash| splash.hint.is_some());
        assert!(idle.composer_inside(), "{cols}x{rows}\n{}", idle.screen());

        terminal.send(DRAFT.as_bytes());
        let typed = terminal.settle("the wrapped draft", |splash| splash.shows("tail"));
        let at = format!("{cols}x{rows} draft\n{}", typed.screen());
        assert!(typed.prompt <= idle.prompt, "the prompt fell: {at}");
        assert_eq!(
            typed.hint, idle.hint,
            "the draft covered the hint: {at}"
        );
        assert!(typed.composer_inside(), "the draft left the window: {at}");

        terminal.send(&[0x7f; DRAFT.len()]);
        terminal.send(b"/");
        let listed = terminal.settle("slash type-ahead", |splash| splash.shows("/compact"));
        let at = format!("{cols}x{rows} type-ahead\n{}", listed.screen());
        assert_eq!(listed.prompt, idle.prompt, "the prompt moved: {at}");
    }
}

#[test]
fn a_resized_splash_keeps_the_draft_inside_the_window() {
    let home = tempfile::tempdir().unwrap();
    let terminal = Terminal::open(home.path(), 223, 67, 9);
    terminal.settle("the splash", |splash| splash.hint.is_some());
    terminal.send(DRAFT.as_bytes());
    terminal.settle("the wrapped draft", |splash| splash.shows("tail"));
    terminal.resize(100, 30);
    let resized = terminal.settle("the resized draft", |splash| {
        splash.shows("tail") && splash.rows.iter().all(|row| row.chars().count() <= 100)
    });
    let at = format!("100x30 after resize\n{}", resized.screen());
    assert!(resized.composer_inside(), "the draft left the window: {at}");

    terminal.send(b" more");
    let typed = terminal.settle("typing after the resize", |splash| {
        splash.shows("tail more")
    });
    let at = format!("100x30 typed after resize\n{}", typed.screen());
    assert_eq!(typed.prompt, resized.prompt, "the prompt moved: {at}");
}

#[test]
fn a_headless_worker_appears_while_the_parent_is_waiting_for_the_model() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (requested, requests) = channel();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for mut stream in listener.incoming().flatten() {
            let mut buffer = [0_u8; 16384];
            let read = stream.read(&mut buffer).unwrap();
            println!(
                "request: {}",
                String::from_utf8_lossy(&buffer[..read])
                    .lines()
                    .next()
                    .unwrap_or_default()
            );
            if buffer[..read].starts_with(b"POST ") && requested.send(()).is_err() {
                return;
            }
            held.push(stream);
        }
    });
    let home = tempfile::tempdir().unwrap();
    let terminal = Terminal::open(home.path(), 120, 40, port);
    terminal.settle("the splash", |splash| splash.hint.is_some());
    terminal.send(b"work\r");
    assert!(
        requests.recv_timeout(Duration::from_secs(30)).is_ok(),
        "parent model request never arrived:\n{}",
        terminal.rows().join("\n")
    );
    let record = std::fs::read_dir(home.path().join("mail/agents"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let parent = record.file_stem().unwrap();
    let lane = home.path().join("lane");
    std::fs::create_dir(&lane).unwrap();
    let mut worker = std::process::Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["-e", "local", "-p", "--json", "hi"])
        .current_dir(&lane)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.path())
        .env("SUMMONR_HOME", home.path())
        .env("SUMMONR_PARENT_SESSION", parent)
        .env("SUMMONR_MAIL", "on")
        .env("SUMMONR_MODELS_URL", "http://127.0.0.1:9/registry.json")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        terminal.settle("the active headless worker", |splash| {
            splash.shows("1 agent") && !splash.shows("Headless")
        })
    }));
    drop(worker.kill());
    worker.wait().unwrap();
    match frame {
        Ok(frame) => println!("{}", frame.screen()),
        Err(error) => std::panic::resume_unwind(error),
    }
}

#[test]
fn the_status_row_shows_work_while_the_first_model_calls_a_tool() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for mut stream in listener.incoming().flatten() {
            let mut buffer = [0_u8; 65536];
            let read = stream.read(&mut buffer).unwrap();
            if !buffer[..read].starts_with(b"POST ") {
                drop(stream.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n"));
                continue;
            }
            let call = r#"{"id":"c","object":"chat.completion.chunk","model":"m","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"run_command","arguments":""}}]}}]}"#;
            let body = format!("data: {call}\n\n");
            let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
            let chunk = format!("{:x}\r\n{body}\r\n", body.len());
            drop(stream.write_all(head.as_bytes()));
            drop(stream.write_all(chunk.as_bytes()));
            drop(stream.flush());
            held.push(stream);
        }
    });
    let home = tempfile::tempdir().unwrap();
    let terminal = Terminal::open(home.path(), 120, 40, port);
    terminal.settle("the splash", |splash| splash.hint.is_some());
    terminal.send(b"list the files\r");
    terminal.settle("the working status row", |splash| {
        splash.shows("Working…")
    });
}
