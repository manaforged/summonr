use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use summonr::{
    Client, EndpointState as State, Result, last_for, list_sessions, resolve_session, splash,
    sweep_worktrees, workflow,
};

use crate::cli::Args;

pub(super) async fn probe_endpoints() -> Result<()> {
    for name in summonr::endpoint_names()? {
        let info = summonr::auth_info(&name).ok();
        let seat = summonr::resolve_endpoint_unauthed(Some(&name), None).ok();
        let model = seat.as_ref().map_or("", |seat| seat.model.as_str());
        let base = seat.as_ref().and_then(|seat| seat.base_url.as_deref());
        let host = base.map_or("", |base| splash::host_of(Some(base)).unwrap_or(base));
        let status = match (&info, summonr::resolve_endpoint(Some(&name), None)) {
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

fn skip_csi() -> Result<()> {
    use std::io::Read as _;
    let mut byte = [0_u8; 1];
    if std::io::stdin().read(&mut byte)? == 0 || byte[0] != b'[' {
        return Ok(());
    }
    loop {
        if std::io::stdin().read(&mut byte)? == 0 {
            return Ok(());
        }
        if (0x40..=0x7e).contains(&byte[0]) {
            return Ok(());
        }
    }
}

fn mask_one() -> Result<()> {
    use std::io::Write as _;
    eprint!("*");
    std::io::stderr().flush()?;
    Ok(())
}

fn unmask_one() -> Result<()> {
    use std::io::Write as _;
    eprint!("\x08 \x08");
    std::io::stderr().flush()?;
    Ok(())
}

fn read_line(prompt: &str) -> Option<String> {
    use std::io::Write as _;
    eprint!("{prompt}: ");
    std::io::stderr().flush().ok()?;
    let mut typed = String::new();
    std::io::stdin().read_line(&mut typed).ok()?;
    Some(typed)
}

fn read_secret(prompt: &str) -> Result<String> {
    use std::io::{Read as _, Write as _};
    if !std::io::stdin().is_terminal() {
        let mut key = String::new();
        std::io::stdin().read_line(&mut key)?;
        return Ok(key);
    }
    eprint!("{prompt}");
    std::io::stderr().flush()?;
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
            0x7f | 0x08 => {
                if key.pop().is_some() {
                    unmask_one()?;
                }
            }
            0x1b => skip_csi()?,
            other if (32..127).contains(&other) => {
                key.push(other as char);
                mask_one()?;
            }
            _ => {}
        }
    };
    drop(raw);
    eprintln!();
    outcome.map(|()| key)
}

pub(super) fn print_login_catalog() -> Result<()> {
    let (rows, errors): (Vec<_>, Vec<_>) = summonr::endpoint_names()?
        .into_iter()
        .map(|name| summonr::auth_info(&name))
        .partition(Result::is_ok);
    let mut errors: Vec<String> = errors
        .into_iter()
        .filter_map(Result::err)
        .map(|error| error.to_string())
        .collect();
    errors.dedup();
    errors.iter().for_each(|error| eprintln!("{error}"));
    let mut rows: Vec<summonr::AuthInfo> = rows.into_iter().flatten().collect();
    rows.sort_by_key(|row| (row.catalog_rank(), row.name.clone()));
    let mut heading = "";
    for row in &rows {
        if row.catalog_heading() != heading {
            heading = row.catalog_heading();
            if heading == "Coding plans" {
                println!("\n{heading}  (Claude Pro/Max, Copilot, Kimi, GLM/Z.AI, OpenCode)");
            } else if heading == "Decisions" {
                println!("\n{heading}  (summonr classify / Jev)");
            } else if heading == "Local" {
                println!("\n{heading}  (no key)");
            } else {
                println!("\n{heading}");
            }
        }
        println!("  {}", row.catalog_line());
    }
    println!("\nsummonr login NAME     store a key (or device login)");
    println!("summonr login NAME --key   force the key prompt");
    Ok(())
}

pub(super) async fn run_login(args: &Args) -> Result<()> {
    if args.endpoint.is_none() {
        return print_login_catalog();
    }
    let name = args.endpoint.as_deref().unwrap_or_default();
    if !args.prompt.is_empty() {
        return Err(summonr::Error::Config(
            "do not put credentials on the command line — `summonr login NAME` asks for them"
                .to_string(),
        ));
    }
    if key_login(name, args.key)? {
        let key = read_secret(&format!("key for {name}: "))?;
        let source = summonr::check_key(name, key.trim()).await?;
        println!(
            "key for {name} stored in {} ({} chars)",
            source.label(),
            key.trim().chars().count()
        );
        return Ok(());
    }
    let endpoint = summonr::resolve_endpoint(args.endpoint.as_deref(), args.model.as_deref())?;
    let flow = summonr::auth_info(name)?.flow;
    match flow {
        summonr::LoginFlow::Browser => {
            println!("signing in to {} through the browser", endpoint.name)
        }
        _ => println!("signing in to {}", endpoint.name),
    }
    let mut prompt = summonr_auth::Prompt::new(
        |line| println!("{line}"),
        |line| {
            Box::pin(async move {
                tokio::task::spawn_blocking(move || read_line(&line))
                    .await
                    .ok()
                    .flatten()
            })
        },
    );
    summonr::Client::login(&endpoint, &mut prompt).await?;
    println!("signed in · {}", endpoint.name);
    Ok(())
}

fn key_login(name: &str, force: bool) -> Result<bool> {
    Ok(force || !summonr::auth_info(name)?.flow.signs_in())
}
