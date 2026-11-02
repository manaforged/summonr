#![allow(
    dead_code,
    reason = "two test crates use different parts of this driver"
)]

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

pub(crate) struct Terminal {
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
    pub(crate) fn open(home: &Path, cols: u16, rows: u16, port: u16) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let child = pair.slave.spawn_command(command(home, port)).unwrap();
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

    pub(crate) fn rows(&self) -> Vec<String> {
        let parser = self.screen.lock().unwrap();
        let (_, cols) = parser.screen().size();
        parser.screen().rows(0, cols).collect()
    }

    pub(crate) fn resize(&self, cols: u16, rows: u16) {
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

    pub(crate) fn send(&self, bytes: &[u8]) {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes).unwrap();
        writer.flush().unwrap();
    }

    pub(crate) fn wait_output(&self, wait: Duration) -> Result<(), RecvTimeoutError> {
        self.output.recv_timeout(wait)
    }

    pub(crate) fn wait(&self, what: &str, ready: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let screen = self.rows().join("\n");
            if ready(&screen) {
                return screen;
            }
            assert!(Instant::now() < deadline, "{what} never showed:\n{screen}");
            let _timed_out = self.wait_output(Duration::from_millis(100));
        }
    }
}

fn command(home: &Path, port: u16) -> CommandBuilder {
    std::fs::write(
            home.join("config.toml"),
            format!("[endpoint.local]\nbase_url = \"http://127.0.0.1:{port}/v1\"\nmodel = \"m\"\napi_key = \"k\"\ncontext_window = 32768\n"),
        )
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
    command
}

fn logged(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.metadata().is_ok_and(|meta| meta.len() > 0))
}

pub(crate) fn hang_up(home: &Path, port: u16) -> Option<u32> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut child = pair.slave.spawn_command(command(home, port)).unwrap();
    drop(pair.slave);
    let logs = home.join("logs");
    let started = Instant::now() + Duration::from_secs(30);
    while Instant::now() < started && !logged(&logs) {
        std::thread::park_timeout(Duration::from_millis(100));
    }
    drop(pair.master);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status.exit_code());
        }
        if Instant::now() > deadline {
            drop(child.kill());
            return None;
        }
        std::thread::park_timeout(Duration::from_millis(50));
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
