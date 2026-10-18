#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const REPLY: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

fn serve(stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut length = 0;
    let mut line = String::new();
    let mut post = false;
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        if line.starts_with("POST") {
            post = true;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
        if line == "\r\n" {
            break;
        }
    }
    let mut body = vec![0; length];
    drop(reader.read_exact(&mut body));
    let (kind, reply) = if post {
        ("text/event-stream", REPLY)
    } else {
        ("application/json", "{\"data\":[{\"id\":\"m\"}]}")
    };
    let mut stream = stream;
    drop(write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
        reply.len()
    ));
}

fn model() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream));
        }
    });
    port
}

fn real_git() -> PathBuf {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join("git"))
        .find(|git| git.is_file())
        .expect("git on PATH")
}

struct Home {
    dir: tempfile::TempDir,
    calls: PathBuf,
}

impl Home {
    fn new(forward: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let calls = dir.path().join("git-calls");
        let tail = if forward {
            format!("exec '{}' \"$@\"", real_git().display())
        } else {
            "exit 1".to_string()
        };
        let shim = bin.join("git");
        std::fs::write(
            &shim,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n{tail}\n",
                calls.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir_all(dir.path().join(".summonr")).unwrap();
        std::fs::write(
            dir.path().join(".summonr/config.toml"),
            format!(
                "[endpoint.local]\nbase_url = \"http://127.0.0.1:{}/v1\"\nmodel = \"m\"\ncontext_window = 200000\n",
                model()
            ),
        )
        .unwrap();
        Self { dir, calls }
    }

    fn prompt(&self, work: &Path) -> Vec<String> {
        drop(std::fs::remove_file(&self.calls));
        let path = format!("{}:/usr/bin:/bin", self.dir.path().join("bin").display());
        let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
            .args(["local", "-p", "say hi"])
            .current_dir(work)
            .env_clear()
            .env("HOME", self.dir.path())
            .env("SUMMONR_HOME", self.dir.path().join(".summonr"))
            .env("PATH", path)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        std::fs::read_to_string(&self.calls)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

#[test]
fn a_prompt_outside_git_starts_no_git() {
    let home = Home::new(false);
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("notes.txt"), "plain folder").unwrap();
    let calls = home.prompt(work.path());
    assert!(
        calls.is_empty(),
        "summonr ran git outside a repository: {calls:?}"
    );
}

#[test]
fn a_text_answer_in_a_repository_lists_the_workspace_once() {
    let home = Home::new(true);
    let work = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        assert!(
            Command::new(real_git())
                .args(args)
                .current_dir(work.path())
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    std::fs::write(work.path().join("a.txt"), "tracked").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-qm", "init"]);
    home.prompt(work.path());
    let calls = home.prompt(work.path());
    let listings = calls
        .iter()
        .filter(|call| call.contains("ls-files"))
        .count();
    assert!(
        listings <= 1,
        "the workspace was listed {listings} times: {calls:?}"
    );
    assert!(
        !calls.iter().any(|call| {
            [
                "--git-common-dir",
                "--absolute-git-dir",
                "--git-path",
                "--max-parents",
            ]
            .iter()
            .any(|flag| call.contains(flag))
        }),
        "repository layout was asked of git: {calls:?}"
    );
}
