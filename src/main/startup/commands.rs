use std::path::{Path, PathBuf};

use summonr::{Result, last_for, list_sessions, resolve_session, sweep_worktrees, workflow};

use crate::cli::{Args, McpCommand};

pub(super) fn session_path(args: &Args) -> Result<PathBuf> {
    match args.prompt.first() {
        Some(id) => resolve_session(id),
        None => last_for(&workspace()?)?
            .map(|info| info.path)
            .ok_or_else(|| {
                summonr::Error::Config("no prior session for this directory".to_string())
            }),
    }
}

pub(super) fn export_session_cmd(args: &Args) -> Result<()> {
    let text = summonr::export_session(&session_path(args)?)?;
    if args.digest {
        print!("{}", summonr::classify::digest::digest(&text));
    } else {
        print!("{text}");
    }
    Ok(())
}

pub(super) fn print_sessions() -> Result<()> {
    let sessions = list_sessions()?;
    if sessions.is_empty() {
        println!("no sessions");
        return Ok(());
    }
    const HEADER: &str = "ID\tNAME\tDIRECTORY\tFIRST PROMPT";
    if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        println!("{HEADER}");
    } else {
        eprintln!("{HEADER}");
    }
    for info in sessions.iter().rev() {
        println!(
            "{}\t{}\t{}\t{}",
            summonr::short(&info.id),
            info.name.as_deref().unwrap_or("-"),
            info.cwd.as_deref().unwrap_or("-"),
            info.preview
        );
    }
    Ok(())
}

const GREP_LINES: usize = 50;
const GREP_WIDTH: usize = 160;
const GREP_TIME: &str = "%Y-%m-%d %H:%M";
const UUID_TIME_SHIFT: u32 = 80;

pub(super) fn grep_sessions(pattern: &str) -> Result<()> {
    let needle = pattern.to_lowercase();
    let sessions: Vec<_> = list_sessions()?.into_iter().rev().collect();
    let width = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let mut shown = 0;
    for batch in sessions.chunks(width) {
        let found: Vec<Vec<String>> = std::thread::scope(|scope| {
            let searches: Vec<_> = batch
                .iter()
                .map(|info| scope.spawn(|| matching(&info.path, &needle)))
                .collect();
            searches
                .into_iter()
                .map(|search| search.join().unwrap_or_default())
                .collect()
        });
        for (info, lines) in batch.iter().zip(found) {
            for line in lines {
                if shown == GREP_LINES {
                    println!("… more matches; narrow the pattern");
                    return Ok(());
                }
                println!(
                    "{}\t{}\t{}\t{}",
                    summonr::short(&info.id),
                    started(&info.id),
                    info.cwd.as_deref().unwrap_or("-"),
                    line
                );
                shown += 1;
            }
        }
    }
    if shown == 0 {
        eprintln!("no matches");
        std::process::exit(1);
    }
    Ok(())
}

fn matching(journal: &Path, needle: &str) -> Vec<String> {
    let Ok(text) = summonr::export_session(journal) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| line.to_lowercase().contains(needle))
        .map(|line| line.trim().chars().take(GREP_WIDTH).collect())
        .collect()
}

fn started(id: &str) -> String {
    u128::from_str_radix(id, 16)
        .ok()
        .and_then(|bits| i64::try_from(bits >> UUID_TIME_SHIFT).ok())
        .and_then(|millis| jiff::Timestamp::from_millisecond(millis).ok())
        .map_or_else(
            || "-".to_string(),
            |at| {
                at.to_zoned(jiff::tz::TimeZone::system())
                    .strftime(GREP_TIME)
                    .to_string()
            },
        )
}

pub(super) fn print_workflows() -> Result<()> {
    let names = workflow::names()?;
    if names.is_empty() {
        println!("no workflows; add [workflows.NAME] to ~/.summonr/config.toml");
        return Ok(());
    }
    let default = workflow::default_name()?;
    for name in names {
        let mark = if default.as_deref() == Some(name.as_str()) {
            "*"
        } else {
            " "
        };
        let seats = workflow::load(&name)
            .map_or_else(|error| error.to_string(), |workflow| workflow.summary());
        println!("{mark} {name}\t{seats}");
    }
    Ok(())
}

pub(super) fn workspace() -> Result<PathBuf> {
    std::env::current_dir().map_err(|error| summonr::Error::Config(error.to_string()))
}

pub(super) fn manage_worktrees(args: &Args) -> Result<()> {
    let cwd = workspace()?;
    if args.prune_worktrees {
        let removed = sweep_worktrees(&cwd)?;
        if args.json {
            println!("{}", serde_json::to_string(&removed)?);
            return Ok(());
        }
        if removed.is_empty() {
            println!("no missing worktrees");
        }
        for path in removed {
            println!("removed {}", path.display());
        }
        return Ok(());
    }
    let trees = summonr::worktree_status(&cwd)?;
    if args.json {
        let rows: Vec<serde_json::Value> = trees
            .iter()
            .map(|row| {
                serde_json::json!({
                    "branch": row.tree.branch,
                    "base": row.tree.base,
                    "path": row.tree.path,
                    "ahead": row.ahead,
                    "dirty": row.dirty,
                    "session": row.session,
                })
            })
            .collect();
        println!("{}", serde_json::to_string(&rows)?);
        return Ok(());
    }
    if trees.is_empty() {
        println!("no worktrees");
        return Ok(());
    }
    for row in trees {
        println!(
            "{}\t+{}\t{}\t{}\t{}",
            row.tree.branch,
            row.ahead,
            if row.dirty { "dirty" } else { "clean" },
            row.session.as_deref().map_or("-", summonr::short),
            row.tree.path.display()
        );
    }
    Ok(())
}

pub(super) fn print_diff(args: &Args) -> Result<()> {
    let cwd = workspace()?;
    let path = args.prompt.first().map(String::as_str);
    let turn = args
        .turn
        .as_deref()
        .map(|turn| session_turn(&cwd, turn))
        .transpose()?;
    if turn.is_none() && !args.json && !args.stat {
        print!("{}", summonr::diff::patch(&cwd, path)?);
        return Ok(());
    }
    let mut diff = match &turn {
        Some((session, turn)) => summonr::diff::turn_stats(&cwd, session, *turn)?,
        None => summonr::diff::stats(&cwd)?,
    };
    diff.files
        .retain(|stat| path.is_none_or(|path| stat.path == path));
    let Some((session, turn)) = turn.filter(|_| !args.json && !args.stat) else {
        return print_stats(&diff, args.json);
    };
    eprintln!("{}", diff.summary());
    print!("{}", summonr::diff::turn_patch(&cwd, &session, turn, path)?);
    Ok(())
}

fn session_turn(cwd: &Path, turn: &str) -> Result<(String, summonr::diff::TurnId)> {
    let turn = turn.parse()?;
    let session = summonr::last_for(cwd)?
        .map(|info| info.id)
        .ok_or_else(|| summonr::Error::Config(format!("no session for {}", cwd.display())))?;
    Ok((session, turn))
}

fn print_stats(diff: &summonr::diff::Diff, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(diff)?);
        return Ok(());
    }
    for stat in &diff.files {
        let new = if stat.new { "  new" } else { "" };
        println!("+{} −{} {}{new}", stat.added, stat.removed, stat.path);
    }
    println!("{}", diff.summary());
    Ok(())
}

pub(super) fn adopt(args: &Args) -> Result<Vec<String>> {
    let moved = summonr::adopt_keys()?;
    if !moved.is_empty() && !args.json {
        eprintln!(
            "moved {} out of config.toml into the credential store",
            moved.join(", ")
        );
    }
    Ok(moved)
}

pub(super) async fn mcp(command: &McpCommand, json_flag: bool) -> Result<()> {
    match command {
        McpCommand::List { json } => mcp_list(*json || json_flag),
        McpCommand::Get { name, json } => mcp_get(name, *json || json_flag),
        McpCommand::Add { name, launch } => {
            let kept = summonr::mcp_add(name, launch)?;
            let count = summonr::mcp_probe(&workspace()?, name)
                .await
                .and_then(|count| {
                    count.ok_or_else(|| {
                        summonr::Error::Config(format!("no MCP server named {name}"))
                    })
                })
                .map_err(|error| {
                    summonr::Error::Setup(format!(
                        "{name}: not reachable: {error}\nkept in {kept}; fix the command or run `summonr mcp remove {name}`"
                    ))
                })?;
            println!("{name}: {count} tools");
            Ok(())
        }
        McpCommand::Remove { name } => {
            if !summonr::mcp_remove(name)? {
                return Err(summonr::Error::Config(format!(
                    "no MCP server named {name} in ~/.summonr/config.toml"
                )));
            }
            println!("removed mcp {name}");
            Ok(())
        }
    }
}

fn mcp_list(json: bool) -> Result<()> {
    let rows = summonr::mcp_status(&workspace()?, None)?;
    if json {
        println!("{}", serde_json::to_string(&rows)?);
    } else if rows.is_empty() {
        println!("no MCP servers configured");
    } else {
        rows.iter().for_each(|row| println!("{}", server_line(row)));
    }
    Ok(())
}

fn mcp_get(name: &str, json: bool) -> Result<()> {
    let row = summonr::mcp_status(&workspace()?, None)?
        .into_iter()
        .find(|row| row.name == name)
        .ok_or_else(|| summonr::Error::Config(format!("no MCP server named {name}")))?;
    if json {
        println!("{}", serde_json::to_string(&row)?);
        return Ok(());
    }
    println!("{}", server_line(&row));
    for text in [&row.launch, &row.detail].into_iter().flatten() {
        println!("{text}");
    }
    row.tools.iter().for_each(|tool| println!("{tool}"));
    Ok(())
}

const STATE_WIDTH: usize = 22;

fn server_line(row: &summonr::ServerStatus) -> String {
    let transport = row.transport.map_or("-", summonr::McpTransport::label);
    let line = format!(
        "{:<20} {:<STATE_WIDTH$} {transport:<6} {} tools  {}",
        row.name,
        row.state_text(STATE_WIDTH),
        row.tools.len(),
        row.schema_size()
    );
    if let (summonr::McpState::Failed, Some(detail)) = (row.state, &row.detail) {
        format!("{line}\n  failed: {detail}")
    } else {
        line
    }
}
