use std::path::{Path, PathBuf};

use summonr::{
    Client, EndpointState as State, Result, last_for, list_sessions, resolve_session, splash,
    sweep_worktrees, workflow,
};

use crate::cli::{Args, McpCommand};

pub(super) async fn probe_endpoints() -> Result<()> {
    if let Err(error) = summonr::catalog::refresh_registry().await {
        eprintln!("summonr: models.dev catalog not refreshed: {error}");
    }
    let names = summonr::endpoint_names_ordered()?;
    let rows = futures::future::join_all(names.iter().map(|name| async move {
        let (model, host, status) = probe_endpoint(name).await;
        (name, model, host, status)
    }))
    .await;
    let width = |column: fn(&(&String, String, String, String)) -> usize| {
        rows.iter().map(column).max().unwrap_or(0)
    };
    let name_w = width(|row| row.0.chars().count());
    let model_w = width(|row| row.1.chars().count());
    let host_w = width(|row| row.2.chars().count());
    for (name, model, host, status) in &rows {
        println!("{name:<name_w$}  {model:<model_w$}  {host:<host_w$}  {status}");
    }
    Ok(())
}

async fn probe_endpoint(name: &str) -> (String, String, String) {
    let info = summonr::auth_info(name).ok();
    let seat = summonr::resolve_endpoint(Some(name), None, summonr::Need::Model).ok();
    let model = seat
        .as_ref()
        .map_or_else(String::new, |seat| seat.model.clone());
    let base = seat.as_ref().and_then(|seat| seat.base_url.clone());
    let host = base.as_deref().map_or_else(String::new, |base| {
        splash::host_of(Some(base)).unwrap_or(base).to_string()
    });
    let status = match (
        &info,
        summonr::resolve_endpoint(Some(name), None, summonr::Need::Seat),
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
                Ok(models) => format!(
                    "{} · {} model{}{key}",
                    State::Ready,
                    models.len(),
                    if models.len() == 1 { "" } else { "s" }
                ),
                Err(error) => format!("{} · {error}{key}", Client::state(&error)),
            }
        }
    };
    (model, host, status)
}

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
    if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        println!("ID\tNAME\tDIRECTORY\tFIRST PROMPT");
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

fn last_session(cwd: &Path) -> Result<String> {
    summonr::last_for(cwd)?
        .map(|info| info.id)
        .ok_or_else(|| summonr::Error::Config(format!("no session for {}", cwd.display())))
}

pub(super) fn print_diff(args: &Args) -> Result<()> {
    let cwd = workspace()?;
    let path = args.prompt.first().map(String::as_str);
    let Some(turn) = args.turn.as_deref() else {
        if args.json || args.stat {
            let mut diff = summonr::diff::stats(&cwd)?;
            diff.files
                .retain(|stat| path.is_none_or(|path| stat.path == path));
            return print_stats(&diff, args.json);
        }
        print!("{}", summonr::diff::patch(&cwd, path)?);
        return Ok(());
    };
    let turn: summonr::diff::TurnId = turn.parse()?;
    let session = last_session(&cwd)?;
    if args.json || args.stat {
        let mut diff = summonr::diff::turn_stats(&cwd, &session, turn)?;
        diff.files
            .retain(|stat| path.is_none_or(|path| stat.path == path));
        return print_stats(&diff, args.json);
    }
    let mut diff = summonr::diff::turn_stats(&cwd, &session, turn)?;
    diff.files
        .retain(|stat| path.is_none_or(|path| stat.path == path));
    eprintln!("{}", diff.summary());
    print!("{}", summonr::diff::turn_patch(&cwd, &session, turn, path)?);
    Ok(())
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
                .map_err(|error| {
                    summonr::Error::Config(format!(
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
    format!(
        "{:<20} {:<STATE_WIDTH$} {transport:<6} {} tools  {}",
        row.name,
        row.state_text(STATE_WIDTH),
        row.tools.len(),
        row.schema_size()
    )
}
