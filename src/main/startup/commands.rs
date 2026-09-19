use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use summonr::{
    Client, Result, last_for, list_sessions, resolve_session, splash, sweep_worktrees, workflow,
};

use crate::cli::Args;

pub(super) async fn probe_endpoints() -> Result<()> {
    for name in summonr::endpoint_names()? {
        let endpoint = match summonr::resolve_endpoint(Some(&name), None) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                println!("{name:<14} unresolved · {error}");
                continue;
            }
        };
        let Some(base) = endpoint.base_url.as_deref() else {
            println!("{name:<14} {:<22} adapter", endpoint.model);
            continue;
        };
        let host = splash::host_of(Some(base)).unwrap_or(base);
        let client = match Client::connect(&endpoint) {
            Ok(client) => client,
            Err(error) => {
                println!("{name:<14} {:<22} {host:<34} {error}", endpoint.model);
                continue;
            }
        };
        let status =
            match tokio::time::timeout(std::time::Duration::from_secs(6), client.models()).await {
                Ok(Ok(_)) => "ok".to_string(),
                Ok(Err(error)) => error.to_string(),
                Err(_) => "timeout".to_string(),
            };
        println!(
            "{name:<14} {:<22} {host:<34} {status} · key: {}",
            endpoint.model,
            endpoint
                .source
                .map_or("none", summonr::secret::Source::label)
        );
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
        print!("{}", summonr::decisions::digest::digest(&text));
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
            summonr::short_session(&info.id),
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
        println!("no workflows — add [workflows.NAME] to ~/.mage/config.toml");
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
            row.session.as_deref().map_or("-", summonr::short_session),
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

struct Raw;

impl Raw {
    fn enter() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        drop(crossterm::terminal::disable_raw_mode());
    }
}

fn read_secret(prompt: &str) -> Result<String> {
    use std::io::Read as _;
    if !std::io::stdin().is_terminal() {
        let mut key = String::new();
        std::io::stdin().read_line(&mut key)?;
        return Ok(key);
    }
    eprint!("{prompt}");
    let raw = Raw::enter()?;
    let mut key = String::new();
    let mut byte = [0_u8; 1];
    let outcome = loop {
        if std::io::stdin().read(&mut byte)? == 0 {
            break Ok(());
        }
        match byte[0] {
            b'\r' | b'\n' => break Ok(()),
            0x03 | 0x04 => break Err(summonr::Error::Config("cancelled".to_string())),
            0x7f | 0x08 => drop(key.pop()),
            other => key.push(other as char),
        }
    };
    drop(raw);
    eprintln!();
    outcome.map(|()| key)
}

pub(super) async fn run_login(args: &Args) -> Result<()> {
    if args.endpoint.is_none() {
        return Err(summonr::Error::Config(
            "login needs an endpoint name, for example `summon login copilot`; `summon endpoints` lists them".to_string(),
        ));
    }
    let name = args.endpoint.as_deref().unwrap_or_default();
    if !args.prompt.is_empty() {
        return Err(summonr::Error::Config(
            "do not put credentials on the command line — `summon login NAME` asks for them"
                .to_string(),
        ));
    }
    if key_login(name, args.key)? {
        let key = read_secret(&format!("key for {name}: "))?;
        let source = summonr::save_key(name, key.trim())?;
        println!("key for {name} stored in {}", source.label());
        return Ok(());
    }
    let endpoint = summonr::resolve_endpoint(args.endpoint.as_deref(), args.model.as_deref())?;
    println!("login {}", endpoint.name);
    summonr::Client::login(&endpoint, |line| println!("{line}")).await?;
    println!("ok");
    Ok(())
}

fn key_login(name: &str, force: bool) -> Result<bool> {
    Ok(force || !summonr::auth_info(name)?.device_login)
}
