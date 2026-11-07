use std::path::Path;

use summonr_core::error::{Error, Result};

use super::{Content, Fill, Panel};

const USAGE: &str = r#"summonr panel list|check NAME

A panel is a tab in the desktop app's right panel. It appears within seconds of writing its file, and its command runs only while its tab is open.

Write panels/NAME.toml under .summonr in the workspace, or under ~/.summonr for every workspace:

  title = "Build times"
  command = ["scripts/build-times.sh"]
  every_secs = 60

The command runs in the workspace and prints one JSON document:

  {"view":"chart","kind":"line","labels":["Mon","Tue"],"series":[{"name":"p50","values":[41.0,38.5]}]}
  {"view":"stat","items":[{"label":"p50","value":"38 ms","tone":"success"}]}
  {"view":"table","columns":["host","state"],"rows":[["web-1","up"]]}
  {"view":"text","text":"all green"}

A chart's kind is line, bar, area or pie. A stat's tone is neutral, success, warning or danger.

With kind = "terminal" in the file, the tab opens the command in a terminal instead.

Run `summonr panel check NAME` after writing a panel. It runs the command and reports what the app cannot draw."#;

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
        [..] => Err(Error::Config(
            "usage: summonr panel list|check NAME; panel --help has the file format".to_string(),
        )),
    }
}

async fn check(name: &str, cwd: &Path, trust: bool) -> Result<i32> {
    let panel = super::list(cwd, trust)
        .into_iter()
        .find(|panel| panel.name == name)
        .ok_or_else(|| Error::Config(format!("no panel named `{name}`")))?;
    if panel.kind == Fill::Terminal {
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
