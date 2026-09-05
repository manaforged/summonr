use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(20);

struct Peer {
    child: Child,
    stdin: ChildStdin,
    lines: std::io::Lines<BufReader<ChildStdout>>,
    next: i64,
}

impl Peer {
    fn spawn(home: &Path, cwd: &Path, env: &[(&str, &str)]) -> Peer {
        let mut command = Command::new(env!("CARGO_BIN_EXE_summon"));
        command
            .args(["room", "serve"])
            .current_dir(cwd)
            .env("MAGE_HOME", home)
            .env_remove("MAGE_BUS_ID")
            .env_remove("MAGE_BUS_KIND")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("spawn summon room serve");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        Peer {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next: 1,
        }
    }

    fn send(&mut self, value: Value) {
        writeln!(self.stdin, "{value}").expect("write");
        self.stdin.flush().expect("flush");
    }

    fn next(&mut self) -> Value {
        loop {
            let line = self.lines.next().expect("eof").expect("read");
            if !line.trim().is_empty() {
                return serde_json::from_str(&line).expect("json");
            }
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let frame = self.next();
            if frame.get("id") == Some(&json!(id)) {
                return frame;
            }
        }
    }

    fn initialize(&mut self, client: &str) -> Value {
        let reply = self.call(
            "initialize",
            json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": client, "version": "0"}}),
        );
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        reply
    }

    fn tool(&mut self, name: &str, args: Value) -> String {
        let reply = self.call("tools/call", json!({"name": name, "arguments": args}));
        reply
            .pointer("/result/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("no text in {reply}"))
            .to_string()
    }

    fn close(mut self) {
        drop(self.stdin);
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if self.child.try_wait().expect("wait").is_some() {
                return;
            }
            pause(50);
        }
        drop(self.child.kill());
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "a process test waits on a child from a plain thread"
)]
fn pause(millis: u64) {
    std::thread::sleep(Duration::from_millis(millis));
}

fn fixture(tag: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("summon-room-{tag}-{}", std::process::id()));
    let home = root.join("home");
    let cwd = root.join("ws");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    (home, cwd)
}

#[test]
fn two_peers_exchange_mail_by_push_and_by_wait() {
    let (home, cwd) = fixture("mail");
    let mut claude = Peer::spawn(&home, &cwd, &[("MAGE_BUS_ID", "claude-a")]);
    let init = claude.initialize("claude-code");
    assert_eq!(
        init.pointer("/result/capabilities/experimental/claude~1channel"),
        Some(&json!({}))
    );
    let mut codex = Peer::spawn(&home, &cwd, &[("MAGE_BUS_ID", "codex-b")]);
    codex.initialize("codex");
    assert!(codex.tool("agents", json!({})).contains("codex-b · codex"));

    let listing = claude.tool("agents", json!({}));
    assert!(listing.contains("claude-a · claude"), "{listing}");
    assert!(listing.contains("codex-b · codex"), "{listing}");

    assert_eq!(
        claude.tool("send", json!({"to": "codex-b", "text": "take the parser"})),
        "sent to 1 agent(s)"
    );
    let waited = codex.tool("wait", json!({"secs": 10}));
    assert_eq!(waited, "[claude-a (claude)] take the parser");

    codex.tool("send", json!({"to": "claude-a", "text": "parser done"}));
    let pushed = claude.next();
    assert_eq!(
        pushed.get("method").and_then(Value::as_str),
        Some("notifications/claude/channel")
    );
    assert_eq!(
        pushed.pointer("/params/content").and_then(Value::as_str),
        Some("parser done")
    );
    assert_eq!(
        pushed.pointer("/params/meta/from").and_then(Value::as_str),
        Some("codex-b")
    );
    assert_eq!(
        claude.tool("inbox", json!({})),
        "[codex-b (codex)] parser done",
        "a push is a copy; the inbox still holds the mail"
    );
    assert_eq!(claude.tool("inbox", json!({})), "no new mail");

    let dropped = claude.call(
        "tools/call",
        json!({"name": "send", "arguments": {"to": "codex-b", "text": "take the parser"}}),
    );
    assert!(
        dropped
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|m| m.contains("same text")),
        "{dropped}"
    );

    assert_eq!(
        codex.tool(
            "board",
            json!({"action": "set", "key": "file:src/a.rs", "value": "codex-b"})
        ),
        "file:src/a.rs = codex-b"
    );
    assert!(
        claude
            .tool("board", json!({"action": "list"}))
            .contains("file:src/a.rs = codex-b  (codex-b)")
    );

    codex.tool("leave", json!({}));
    assert!(!claude.tool("agents", json!({})).contains("codex-b"));
    codex.close();
    claude.close();
    let roster = Command::new(env!("CARGO_BIN_EXE_summon"))
        .arg("room")
        .env("MAGE_HOME", &home)
        .output()
        .expect("roster");
    assert_eq!(
        String::from_utf8_lossy(&roster.stdout).trim(),
        "the room is empty"
    );
    drop(std::fs::remove_dir_all(home.parent().expect("root")));
}

#[test]
fn a_client_without_push_reads_mail_from_inbox() {
    let (home, cwd) = fixture("inbox");
    let mut a = Peer::spawn(&home, &cwd, &[]);
    a.initialize("codex");
    let joined = a.tool("join", json!({"id": "worker", "kind": "codex"}));
    assert!(joined.starts_with("joined as worker"), "{joined}");
    let mut b = Peer::spawn(
        &home,
        &cwd,
        &[("MAGE_BUS_ID", "lead"), ("MAGE_ROOM_PUSH", "off")],
    );
    b.initialize("claude-code");
    assert!(b.tool("agents", json!({})).contains("lead · claude"));
    a.tool("send", json!({"to": "lead", "text": "hello"}));
    assert_eq!(b.tool("inbox", json!({})), "[worker (codex)] hello");
    assert_eq!(b.tool("wait", json!({"secs": 1})), "no mail in 1s");
    a.close();
    b.close();
    drop(std::fs::remove_dir_all(home.parent().expect("root")));
}

#[test]
fn the_codex_host_answers_mail_on_one_thread() {
    let (home, cwd) = fixture("host");
    let fake = cwd.join("codex.py");
    std::fs::write(
        &fake,
        r#"
import sys, json
if sys.argv[1:] != ["mcp-server"]:
    sys.exit(2)
calls = []
for line in sys.stdin:
    m = json.loads(line)
    method = m.get("method")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":m["id"],"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}}}}), flush=True)
    elif method == "tools/call":
        name = m["params"]["name"]; args = m["params"]["arguments"]
        calls.append((name, args.get("threadId")))
        print(json.dumps({"jsonrpc":"2.0","method":"codex/event","params":{"msg":{"type":"exec_command_begin","command":["ls"]}}}), flush=True)
        text = "%s#%d thread=%s says %s" % (name, len(calls), args.get("threadId"), args["prompt"])
        print(json.dumps({"jsonrpc":"2.0","id":m["id"],"result":{"structuredContent":{"threadId":"t-1","content":text},"content":[{"type":"text","text":text}]}}), flush=True)
"#,
    )
    .expect("fake codex");
    let wrapper = cwd.join("codex");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\nexec python3 {} \"$@\"\n", fake.display()),
    )
    .expect("wrapper");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let mut host = Command::new(env!("CARGO_BIN_EXE_summon"))
        .args(["room", "host", "codex", "--id", "codex-pair", "--read-only"])
        .current_dir(&cwd)
        .env("MAGE_HOME", &home)
        .env("MAGE_CODEX", &wrapper)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn host");

    let mut lead = Peer::spawn(
        &home,
        &cwd,
        &[("MAGE_BUS_ID", "lead"), ("MAGE_ROOM_PUSH", "off")],
    );
    lead.initialize("claude-code");
    let deadline = Instant::now() + WAIT;
    while !lead
        .tool("agents", json!({}))
        .contains("codex-pair · codex")
    {
        assert!(Instant::now() < deadline, "the host never joined");
        pause(100);
    }
    lead.tool("send", json!({"to": "codex-pair", "text": "one"}));
    let first = lead.tool("wait", json!({"secs": 15}));
    assert_eq!(
        first,
        "[codex-pair (codex)] codex#1 thread=None says [lead (claude)] one"
    );
    lead.tool("send", json!({"to": "codex-pair", "text": "two"}));
    let second = lead.tool("wait", json!({"secs": 15}));
    assert_eq!(
        second,
        "[codex-pair (codex)] codex-reply#2 thread=t-1 says [lead (claude)] two"
    );

    drop(host.kill());
    drop(host.wait());
    lead.close();
    drop(std::fs::remove_dir_all(home.parent().expect("root")));
}
