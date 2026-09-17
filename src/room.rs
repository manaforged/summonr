use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::bus;
use crate::durable::store;
use crate::error::{Error, Result};

mod host;
mod serve;

/// A short room id. One copy: two files minted the same four hex digits.
pub(super) fn short() -> String {
    let seed = std::process::id() as u64 ^ bus::now();
    format!("{:04x}", seed & 0xffff)
}

const SKILL: &str = include_str!("room/SKILL.md");

const USAGE: &str = "usage:\n  summonr room                 who is in the room\n  summonr room serve           MCP server over stdio (Claude Code and Codex spawn this)\n  summonr room host codex [--cwd PATH] [--id NAME] [--read-only]\n                              run Codex as a peer that answers mail\n  summonr room setup claude    install the /summon-room skill and the .mcp.json entry here\n  summonr room setup codex     install the skill and the ~/.codex/config.toml entry";

pub async fn run(args: &[String], cwd: &Path) -> Result<i32> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => {
            println!("{}", roster()?);
            Ok(0)
        }
        ["serve"] => serve::serve(cwd).await.map(|()| 0),
        ["host", "codex", rest @ ..] => {
            let options = host::Options::parse(rest, cwd)?;
            host::codex(options).await.map(|()| 0)
        }
        ["setup", "claude"] => {
            println!(
                "{}",
                setup_claude(cwd, &home("CLAUDE_CONFIG_DIR", ".claude")?)?
            );
            Ok(0)
        }
        ["setup", "codex"] => {
            println!("{}", setup_codex(&home("CODEX_HOME", ".codex")?)?);
            Ok(0)
        }
        _ => Err(Error::Config(USAGE.to_string())),
    }
}

fn roster() -> Result<String> {
    let agents = bus::peek()?;
    if agents.is_empty() {
        return Ok("the room is empty".to_string());
    }
    Ok(agents
        .iter()
        .map(|agent| {
            format!(
                "{:<20} {:<9} {:<12} {}  {}",
                agent.id, agent.kind, agent.model, agent.cwd, agent.status
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

fn exe() -> Result<String> {
    Ok(std::env::current_exe()?.display().to_string())
}

fn home(var: &str, fallback: &str) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(var) {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| Error::Config("the home directory is unknown".to_string()))?;
    Ok(PathBuf::from(home).join(fallback))
}

fn install_skill(root: &Path) -> Result<PathBuf> {
    let path = root.join("skills/summon-room/SKILL.md");
    store::write(&path, SKILL)?;
    Ok(path)
}

fn setup_claude(cwd: &Path, root: &Path) -> Result<String> {
    let skill = install_skill(root)?;
    let path = cwd.join(".mcp.json");
    let mut config: Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| Error::Config(format!("{} is not JSON: {error}", path.display())))?,
        Err(_) => json!({}),
    };
    let servers = config
        .as_object_mut()
        .ok_or_else(|| Error::Config(format!("{} is not a JSON object", path.display())))?
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    let entry = json!({"type": "stdio", "command": exe()?, "args": ["room", "serve"]});
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| Error::Config("mcpServers is not an object".to_string()))?;
    let changed = servers.get("summon-room") != Some(&entry);
    servers.insert("summon-room".to_string(), entry);
    if changed {
        store::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&config)?),
        )?;
    }
    Ok(format!(
        "skill      {}\nmcp        {} ({})\n\nnext:\n  claude --dangerously-load-development-channels server:summon-room\n  /summon-room\n\nWithout the channel flag the room still works; mail then arrives through the wait tool.",
        skill.display(),
        path.display(),
        if changed { "written" } else { "unchanged" }
    ))
}

fn setup_codex(root: &Path) -> Result<String> {
    let skill = install_skill(root)?;
    let path = root.join("config.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|error| Error::Config(format!("{} is not TOML: {error}", path.display())))?;
    let mut table = toml_edit::Table::new();
    table["command"] = toml_edit::value(exe()?);
    let mut args = toml_edit::Array::new();
    args.push("room");
    args.push("serve");
    table["args"] = toml_edit::value(args);
    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let before = doc.to_string();
    doc["mcp_servers"]["summon-room"] = toml_edit::Item::Table(table);
    let after = doc.to_string();
    let changed = after != before;
    if changed {
        store::write(&path, &after)?;
    }
    Ok(format!(
        "skill      {}\nmcp        {} ({})\n\nCodex cannot receive a push. Inside Codex, call the wait tool to read the room, or let Claude Code host Codex with `summonr room host codex`.",
        skill.display(),
        path.display(),
        if changed { "written" } else { "unchanged" }
    ))
}
