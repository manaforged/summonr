use std::path::Path;

use summonr_core::error::{Error, Result};

use super::{Content, Kind, Panel};

const USAGE: &str = "summonr panel list|check NAME";

/// Runs `summonr panel`, with the workspace's panels when it is trusted.
pub async fn run(words: &[String], cwd: &Path, trust: bool) -> Result<i32> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["list"] => {
            for panel in super::list(cwd, trust) {
                println!("{}\t{}", panel.name, panel.title);
            }
            Ok(0)
        }
        ["check", name] => check(name, cwd, trust).await,
        ["-h" | "--help"] => {
            println!("{USAGE}");
            Ok(0)
        }
        _ => Err(Error::Config(format!("usage: {USAGE}"))),
    }
}

async fn check(name: &str, cwd: &Path, trust: bool) -> Result<i32> {
    let panel = super::list(cwd, trust)
        .into_iter()
        .find(|panel| panel.name == name)
        .ok_or_else(|| Error::Config(format!("no panel named `{name}`")))?;
    if panel.kind == Kind::Terminal {
        return checked_terminal(&panel);
    }
    let content = super::run(&panel, cwd)
        .await
        .map_err(|reason| Error::Config(format!("panel `{name}`: {reason}")))?;
    println!("ok: {name} draws {}", shape(&content));
    Ok(0)
}

fn checked_terminal(panel: &Panel) -> Result<i32> {
    if let Some(problem) = &panel.problem {
        return Err(Error::Config(format!("panel `{}`: {problem}", panel.name)));
    }
    println!("ok: {} opens a terminal", panel.name);
    Ok(0)
}

fn shape(content: &Content) -> String {
    match content {
        Content::Chart { series, .. } => format!("a chart of {} series", series.len()),
        Content::Stat { items } => format!("{} stats", items.len()),
        Content::Table { rows, .. } => format!("a table of {} rows", rows.len()),
        Content::Text { .. } => "text".to_string(),
    }
}
