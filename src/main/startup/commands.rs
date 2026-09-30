use std::path::{Path, PathBuf};

use summonr::{
    Client, EndpointState as State, Result, last_for, list_sessions, resolve_session, splash,
    sweep_worktrees, workflow,
};

use crate::cli::{Args, McpCommand};

pub(super) async fn probe_endpoints() -> Result<()> {
    for name in summonr::endpoint_names()? {
        let info = summonr::auth_info(&name).ok();
        let seat = summonr::resolve_endpoint(Some(&name), None, summonr::Need::Model).ok();
        let model = seat.as_ref().map_or("", |seat| seat.model.as_str());
        let base = seat.as_ref().and_then(|seat| seat.base_url.as_deref());
        let host = base.map_or("", |base| splash::host_of(Some(base)).unwrap_or(base));
        let status = match (
            &info,
            summonr::resolve_endpoint(Some(&name), None, summonr::Need::Seat),
        ) {
            (Some(info), _) if !info.configured && !info.keyless() => {
                format!("{} · {}", info.state(), info.setup)
            }
            (Some(_), Err(_)) if seat.is_none() => format!(
                "{} · run `summonr {name} --model NAME` or set model in config.toml",
                State::NeedsModel
            ),
            (_, Err(error)) => format!("{} · {error}", State::Error),
            (_, Ok(endpoint)) if endpoint.base_url.is_none() => "adapter".to_string(),
            (_, Ok(endpoint)) => {
                let key = endpoint
                    .source
                    .map_or_else(String::new, |source| format!(" · key: {}", source.label()));
                match Client::discover(&endpoint).await {
                    Ok(models) => format!("{} · {} models{key}", State::Ready, models.len()),
                    Err(error) => format!("{} · {error}{key}", Client::state(&error)),
                }
            }
        };
        println!("{name:<14} {model:<22} {host:<34} {status}");
    }
    Ok(())
}

pub(super) fn session_path(args: &Args) -> Result<PathBuf> {
    match args.prompt.first() {
        Some(id) => resolve_session(id),
        None => last_for(&workspace()?)?.ok_or_else(|| {
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

pub(super) fn print_workflows() -> Result<()> {
    let names = workflow::names()?;
    if names.is_empty() {
        println!("no workflows — add [workflows.NAME] to ~/.summonr/config.toml");
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

fn turn_paths(cwd: &Path, turn: &str) -> Result<Vec<String>> {
    let journal = summonr::last_for(cwd)?
        .ok_or_else(|| summonr::Error::Config(format!("no session for {}", cwd.display())))?;
    let turns = summonr::diff::turns(&journal)?;
    let index = if turn == "last" {
        turns.len().checked_sub(1)
    } else {
        turn.parse::<usize>().ok().and_then(|n| n.checked_sub(1))
    };
    index
        .and_then(|index| turns.get(index).cloned())
        .ok_or_else(|| {
            summonr::Error::Config(format!(
                "no turn `{turn}` — the session has {} turn{}",
                turns.len(),
                if turns.len() == 1 { "" } else { "s" }
            ))
        })
}

pub(super) fn print_diff(args: &Args) -> Result<()> {
    let cwd = workspace()?;
    let path = args.prompt.first().map(String::as_str);
    let only = match args.turn.as_deref() {
        Some(turn) => Some(turn_paths(&cwd, turn)?),
        None => None,
    };
    let keep = |candidate: &str| {
        path.is_none_or(|path| candidate == path)
            && only
                .as_ref()
                .is_none_or(|only| only.iter().any(|file| file == candidate))
    };
    if args.json {
        let mut diff = summonr::diff::stats(&cwd)?;
        diff.files.retain(|stat| keep(&stat.path));
        println!("{}", serde_json::to_string(&diff)?);
        return Ok(());
    }
    if args.stat {
        let mut diff = summonr::diff::stats(&cwd)?;
        diff.files.retain(|stat| keep(&stat.path));
        for stat in &diff.files {
            let new = if stat.new { "  new" } else { "" };
            println!("+{:<5} −{:<5} {}{new}", stat.added, stat.removed, stat.path);
        }
        println!("{}", diff.summary());
        return Ok(());
    }
    match only {
        None => print!("{}", summonr::diff::patch(&cwd, path)?),
        Some(only) => {
            for file in only
                .iter()
                .filter(|file| path.is_none_or(|path| path == *file))
            {
                print!("{}", summonr::diff::patch(&cwd, Some(file))?);
            }
        }
    }
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

pub(super) fn mcp(command: &McpCommand) -> Result<()> {
    match command {
        McpCommand::List { json } => mcp_list(*json),
        McpCommand::Get { name, json } => mcp_get(name, *json),
        McpCommand::Add { name, launch } => {
            summonr::mcp_add(name, launch)?;
            println!("added mcp {name}; restart summonr to use it");
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

fn server_line(row: &summonr::ServerStatus) -> String {
    let transport = row.transport.map_or("-", summonr::McpTransport::label);
    format!(
        "{:<20} {:<22} {transport:<6} {} tools  {}",
        row.name,
        row.state.label(),
        row.tools.len(),
        row.schema_size()
    )
}
