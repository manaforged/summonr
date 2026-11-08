use std::path::Path;

use summonr_core::error::{Error, Result};

use super::{Content, Fill, Panel};

const USAGE: &str = r#"summonr panel list|check NAME

A panel is a tab in the desktop app's right panel, or a section of its sidebar. It appears within seconds of writing its file. A tab's command runs only while the tab is open; a sidebar section's command runs while the sidebar shows.

Write panels/NAME.toml under .summonr in the workspace, or under ~/.summonr for every workspace:

  title = "Build times"
  command = ["scripts/build-times.sh"]
  every_secs = 60

The command runs in the workspace and prints one JSON document:

  {"view":"chart","kind":"line","labels":["Mon","Tue"],"series":[{"name":"p50","values":[41.0,38.5]}]}
  {"view":"stat","items":[{"label":"p50","value":"38 ms","tone":"success"}]}
  {"view":"table","columns":["host","state"],"rows":[["web-1","up"]]}
  {"view":"text","text":"all green"}
  {"view":"list","rows":[{"title":"Fix the cart total","meta":"issue 212","tone":"warning","action":{"start":{"prompt":"Fix issue 212"}}}]}

A chart's kind is line, bar, area or pie. A stat's or a row's tone is neutral, success, warning or danger. A row's action starts a new agent with a prompt, or opens one: {"open":{"session":"ID"}}.

It can print an A2UI v0.9.1 surface instead, with a component whose id is "root". A Button whose action.event is named start, with a prompt in its context, opens an agent with that prompt; one named open, with a session, shows that agent. Any other event runs the command again with the A2UI action message on stdin, and what it prints replaces the surface:

  [{"version":"v0.9.1","updateComponents":{"surfaceId":"queue","components":[{"id":"root","component":"Button","child":"label","action":{"event":{"name":"start","context":{"prompt":"Review PR 12"}}}},{"id":"label","component":"Text","text":"Review PR 12"}]}}]

With place = "side" in the file, the panel is a section of the sidebar instead of a tab. Commit the file in the workspace's panels folder to share it with everyone who works in the repository.

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
    let content = super::run(&panel, cwd, None)
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
        Content::List { rows } => format!("a list of {} rows", rows.len()),
        Content::Surface(_) => "an A2UI surface; the desktop checks its components".to_string(),
    }
}
