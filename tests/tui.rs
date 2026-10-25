#![cfg(unix)]

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

const SIZES: [(u16, u16); 3] = [(100, 30), (120, 40), (223, 67)];
const DRAFT: &str = "breath of fire pixel style breath of fire pixel style breath of fire pixel style breath of fire pixel style breath of fire pixel style breath of fire pixel style tail";

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

struct Terminal {
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    writer: Writer,
    screen: Arc<Mutex<vt100::Parser>>,
    output: Receiver<()>,
}

impl Drop for Terminal {
    fn drop(&mut self) {
        drop(self.child.kill());
    }
}

impl Terminal {
    fn open(home: &Path, cols: u16, rows: u16, port: u16) -> Self {
        std::fs::write(
            home.join("config.toml"),
            format!("[endpoint.local]\nbase_url = \"http://127.0.0.1:{port}/v1\"\nmodel = \"m\"\napi_key = \"k\"\ncontext_window = 32768\n"),
        )
        .unwrap();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_summonr"));
        command.args(["-e", "local"]);
        command.cwd(home);
        command.env_clear();
        for (key, value) in [
            ("PATH", "/usr/bin:/bin"),
            ("LANG", "en_US.UTF-8"),
            ("TERM", "xterm-256color"),
            ("RUST_MIN_STACK", "16777216"),
            ("SUMMONR_MODELS_URL", "http://127.0.0.1:9/registry.json"),
        ] {
            command.env(key, value);
        }
        for key in ["HOME", "SUMMONR_HOME"] {
            command.env(key, home);
        }
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer: Writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let (parser, replies) = (Arc::clone(&screen), Arc::clone(&writer));
        let (painted, output) = channel();
        std::thread::spawn(move || {
            let mut chunk = [0_u8; 65536];
            while let Ok(read) = reader.read(&mut chunk) {
                let Some(bytes) = chunk.get(..read).filter(|bytes| !bytes.is_empty()) else {
                    break;
                };
                let reply = answer(&mut parser.lock().unwrap(), bytes);
                if !reply.is_empty() {
                    drop(replies.lock().unwrap().write_all(&reply));
                }
                if painted.send(()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            master: pair.master,
            writer,
            screen,
            output,
        }
    }

    fn rows(&self) -> Vec<String> {
        let parser = self.screen.lock().unwrap();
        let (_, cols) = parser.screen().size();
        parser.screen().rows(0, cols).collect()
    }

    fn resize(&self, cols: u16, rows: u16) {
        self.screen
            .lock()
            .unwrap()
            .screen_mut()
            .set_size(rows, cols);
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
    }

    fn send(&self, bytes: &[u8]) {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes).unwrap();
        writer.flush().unwrap();
    }

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
            let wait = self.output.recv_timeout(steady.saturating_sub(held));
            assert_ne!(
                wait,
                Err(RecvTimeoutError::Disconnected),
                "summonr exited before {what} showed:\n{}",
                rows.join("\n")
            );
        }
    }
}

fn answer(parser: &mut vt100::Parser, bytes: &[u8]) -> Vec<u8> {
    let mut reply = Vec::new();
    let mut rest = bytes;
    while let Some(at) = rest.windows(4).position(|window| window == b"\x1b[6n") {
        parser.process(&rest[..at]);
        let (row, col) = parser.screen().cursor_position();
        reply.extend(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes());
        rest = &rest[at + 4..];
    }
    if rest.windows(3).any(|window| window == b"\x1b[c") {
        reply.extend(b"\x1b[?62;22c");
    }
    parser.process(rest);
    reply
}

#[derive(Debug)]
struct Splash {
    rows: Vec<String>,
    card_top: Option<usize>,
    card_bottom: Option<usize>,
    prompt: usize,
    workdir: Option<usize>,
}

impl Splash {
    fn read(rows: &[String]) -> Option<Self> {
        Some(Self {
            rows: rows.to_vec(),
            card_top: rows.iter().position(|row| row.contains('╭')),
            card_bottom: rows.iter().position(|row| row.contains('╰')),
            prompt: rows.iter().rposition(|row| row.contains('❯'))?,
            workdir: rows.iter().position(|row| row.contains("workdir")),
        })
    }

    fn place(&self) -> [Option<usize>; 3] {
        [self.card_bottom, Some(self.prompt), self.workdir]
    }

    fn composer_inside(&self) -> bool {
        self.card_top.is_some_and(|top| top < self.prompt)
            && self.card_bottom.is_some_and(|bottom| self.prompt < bottom)
            && self.workdir.is_some_and(|row| row < self.prompt)
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
    terminal.settle("the splash", |splash| splash.workdir.is_some());
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
        let idle = terminal.settle("the splash", |splash| splash.workdir.is_some());
        assert!(idle.composer_inside(), "{cols}x{rows}\n{}", idle.screen());

        terminal.send(DRAFT.as_bytes());
        let typed = terminal.settle("the wrapped draft", |splash| splash.shows("tail"));
        let at = format!("{cols}x{rows} draft\n{}", typed.screen());
        assert_eq!(typed.prompt, idle.prompt, "the prompt moved: {at}");
        assert_eq!(typed.card_top, idle.card_top, "the window moved: {at}");
        assert_eq!(
            typed.workdir, idle.workdir,
            "the draft covered the card: {at}"
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
    terminal.settle("the splash", |splash| splash.workdir.is_some());
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
    terminal.settle("the splash", |splash| splash.workdir.is_some());
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
            splash.shows("Subagents") && splash.shows("Headless") && splash.shows("lane-1")
        })
    }));
    drop(worker.kill());
    worker.wait().unwrap();
    match frame {
        Ok(frame) => println!("{}", frame.screen()),
        Err(error) => std::panic::resume_unwind(error),
    }
}
