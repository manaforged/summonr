use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::serve::TICK;
use super::short;
use crate::bus::Bus;
use crate::error::{Error, Result};
use crate::rpc::{self, Client};

const BRIEF: &str = "You are one agent in a room with other coding agents on this machine. Each message you receive is mail from a peer, prefixed with the sender's id and kind. Your reply is sent back to that peer as mail, so answer them directly and concisely; put findings, diffs, and file paths in the reply. You may edit files and run commands in your working directory within your sandbox. Do not wait for a human.";

pub(super) struct Options {
    cwd: PathBuf,
    id: Option<String>,
    sandbox: &'static str,
}

impl Options {
    pub(super) fn parse(args: &[&str], cwd: &Path) -> Result<Self> {
        let mut options = Self {
            cwd: cwd.to_path_buf(),
            id: None,
            sandbox: "workspace-write",
        };
        let mut input = args.iter();
        while let Some(arg) = input.next() {
            match *arg {
                "--cwd" => {
                    let path = input
                        .next()
                        .ok_or_else(|| Error::Config("--cwd needs a path".to_string()))?;
                    options.cwd = cwd.join(path);
                }
                "--id" => {
                    let id = input
                        .next()
                        .ok_or_else(|| Error::Config("--id needs a name".to_string()))?;
                    options.id = Some((*id).to_string());
                }
                "--read-only" => options.sandbox = "read-only",
                other => {
                    return Err(Error::Config(format!("unknown option `{other}`")));
                }
            }
        }
        Ok(options)
    }
}

pub(super) async fn codex(options: Options) -> Result<()> {
    let program = std::env::var("SUMMONR_CODEX").unwrap_or_else(|_| "codex".to_string());
    let mut client = Client::spawn(&program, &["mcp-server"], &[], &options.cwd)?;
    client.initialize("summon-room").await?;
    let id = options
        .id
        .clone()
        .unwrap_or_else(|| format!("codex-{}", short()));
    let mut bus = Bus::open(&id, "codex", "codex", &options.cwd)?;
    let id = bus.id().to_string();
    eprintln!(
        "{id} joined the room in {}; ctrl-c leaves",
        options.cwd.display()
    );
    let mut thread: Option<String> = None;
    let mut tick = tokio::time::interval(TICK);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = tokio::signal::ctrl_c() => break,
        }
        bus.status("codex", &options.cwd, "idle");
        for mail in bus.poll()? {
            eprintln!("← {} ({}): {}", mail.from, mail.kind, first(&mail.text));
            bus.status("codex", &options.cwd, &format!("working for {}", mail.from));
            let prompt = format!("[{} ({})] {}", mail.from, mail.kind, mail.text);
            let reply = match answer(&mut client, &options, &mut thread, &prompt).await {
                Ok(text) => text,
                Err(error) => format!("(codex failed: {error})"),
            };
            eprintln!("→ {}: {}", mail.from, first(&reply));
            bus.send(&mail.from, &reply)?;
        }
    }
    bus.leave();
    client.shutdown().await;
    Ok(())
}

async fn answer(
    client: &mut Client,
    options: &Options,
    thread: &mut Option<String>,
    prompt: &str,
) -> Result<String> {
    let progress = |method: &str, params: &Value| {
        if method != "codex/event" {
            return;
        }
        if let Some(command) = params.pointer("/msg/command").and_then(Value::as_array) {
            let line: Vec<&str> = command.iter().filter_map(Value::as_str).collect();
            eprintln!("  $ {}", line.join(" "));
        }
    };
    let result = match thread {
        Some(id) => {
            client
                .tool(
                    "codex-reply",
                    json!({"threadId": id, "prompt": prompt}),
                    progress,
                )
                .await?
        }
        None => {
            let result = client
                .tool(
                    "codex",
                    json!({
                        "prompt": prompt,
                        "cwd": options.cwd.display().to_string(),
                        "sandbox": options.sandbox,
                        "approval-policy": "never",
                        "developer-instructions": BRIEF
                    }),
                    progress,
                )
                .await?;
            *thread = result
                .pointer("/structuredContent/threadId")
                .and_then(Value::as_str)
                .map(str::to_string);
            result
        }
    };
    let text = rpc::text(&result);
    Ok(if text.trim().is_empty() {
        "(codex returned no text)".to_string()
    } else {
        text
    })
}

fn first(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(120)
        .collect()
}
