use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use summonr::workflow;
use summonr::{
    Budget, Client, Endpoint, Event, Loop, Result, add_worktree, finish_worktree, in_git, last_for,
    list_sessions, list_worktrees, resolve_endpoint, resolve_session, splash, sweep_worktrees,
    system_prompt_with,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::cli::{self, Args};
use super::{diagnostics, modes};

type Ui = (
    Option<UnboundedSender<summonr::tui::Msg>>,
    Option<UnboundedReceiver<summonr::tui::Msg>>,
);
enum SessionAction {
    Create,
    Resume(PathBuf),
    Fork(PathBuf),
}

enum Launch {
    Done(i32),
    Resume(String),
}

async fn probe_endpoints() -> Result<()> {
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

fn session_path(args: &Args) -> Result<PathBuf> {
    match args.prompt.first() {
        Some(id) => resolve_session(id),
        None => last_for(&workspace()?)?.ok_or_else(|| {
            summonr::Error::Config("no prior session for this directory".to_string())
        }),
    }
}

fn export_session_cmd(args: &Args) -> Result<()> {
    let text = summonr::export_session(&session_path(args)?)?;
    if args.digest {
        print!("{}", summonr::decisions::digest::digest(&text));
    } else {
        print!("{text}");
    }
    Ok(())
}

fn print_sessions() -> Result<()> {
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

fn print_workflows() -> Result<()> {
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

fn workflow_name(args: &Args, loop_: &Loop<Client>) -> Result<Option<String>> {
    if let Some(name) = &args.workflow {
        return Ok(Some(name.clone()));
    }
    if let Some(name) = loop_.workflow() {
        return Ok(Some(name));
    }
    workflow::default_name()
}

fn apply_workflow(
    loop_: &mut Loop<Client>,
    name: &str,
    ui: Option<&UnboundedSender<summonr::tui::Msg>>,
    json: bool,
) -> Result<workflow::Workflow> {
    let workflow = workflow::load(name)?;
    loop_.apply_workflow(&workflow, |seat| {
        let endpoint = resolve_endpoint(Some(&seat.endpoint), seat.model.as_deref())?;
        let client = provider(&endpoint, ui, json)?;
        Ok((client, endpoint))
    })?;
    Ok(workflow)
}

fn workspace() -> Result<PathBuf> {
    std::env::current_dir().map_err(|error| summonr::Error::Config(error.to_string()))
}

fn isolate(args: &Args, cwd: &Path) -> Result<(PathBuf, Option<summonr::Tree>, SessionAction)> {
    let action = load_session(args, cwd)?;
    match &action {
        SessionAction::Resume(path) | SessionAction::Fork(path) => {
            let workspace = journal_workspace(path).unwrap_or_else(|| cwd.to_path_buf());
            let tree = list_worktrees(&workspace)
                .ok()
                .and_then(|trees| trees.into_iter().find(|tree| tree.path == workspace));
            Ok((workspace, tree, action))
        }
        SessionAction::Create if args.worktree => {
            if !in_git(cwd) {
                if !args.json {
                    eprintln!("not a git repository; running in {}", cwd.display());
                }
                return Ok((cwd.to_path_buf(), None, action));
            }
            let tree = add_worktree(cwd)?;
            if !args.json {
                println!("worktree {} ({})", tree.path.display(), tree.branch);
            }
            Ok((tree.path.clone(), Some(tree), action))
        }
        SessionAction::Create => Ok((cwd.to_path_buf(), None, action)),
    }
}

fn journal_workspace(path: &Path) -> Option<PathBuf> {
    list_sessions()
        .ok()?
        .into_iter()
        .find(|info| info.path == path)?
        .cwd
        .map(PathBuf::from)
}

fn manage_worktrees(args: &Args) -> Result<()> {
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

fn print_diff(args: &Args) -> Result<()> {
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

fn adopt(args: &Args) -> Result<Vec<String>> {
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

async fn run_login(args: &Args) -> Result<()> {
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

pub(super) async fn run() -> Result<i32> {
    let args = cli::parse()?;
    let log = if args.room.is_some() {
        None
    } else {
        diagnostics::Log::start()
    };
    let result = route(args, log.as_ref()).await;
    let (code, error) = match &result {
        Ok(code) => (*code, None),
        Err(error) => (1, Some(error.to_string())),
    };
    if let Some(log) = log {
        log.finish(code, error.as_deref());
    }
    result
}

async fn run_diff_review(
    args: &Args,
    asked: &std::collections::BTreeMap<String, summonr::decisions::Question>,
    floor: f64,
) -> Result<i32> {
    let questions = if asked.is_empty() {
        summonr::decisions::sets::diff()
    } else {
        asked.clone()
    };
    let patch = if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        summonr::diff::patch(&workspace()?, None)?
    } else {
        let mut piped = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut piped)?;
        piped
    };
    let changes = summonr::decisions::patch::split(&patch);
    if changes.is_empty() {
        println!("no changes to review");
        return Ok(0);
    }
    let endpoint = decisions_endpoint(args)?;
    let room = endpoint
        .context_window
        .map_or(summonr::decisions::CONTEXT_CHARS, |tokens| {
            tokens as usize * 3
        });
    let asked_size = serde_json::to_string(&questions).map_or(0, |text| text.len());
    let budget = room.saturating_sub(asked_size);
    let decided =
        summonr::decisions::patch::review(&endpoint, &changes, &questions, budget).await?;
    if args.json {
        let rows: Vec<_> = changes
            .iter()
            .zip(&decided)
            .map(|(change, decision)| {
                serde_json::json!({"path": change.path, "answers": decision.answers})
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        print!(
            "{}",
            summonr::decisions::patch::table(&changes, &decided, floor)
        );
    }
    Ok(0)
}

async fn run_eval(
    args: &Args,
    questions: &std::collections::BTreeMap<String, summonr::decisions::Question>,
    path: &str,
    floor: f64,
) -> Result<i32> {
    if questions.is_empty() {
        return Err(summonr::Error::Config(
            "an evaluation needs questions: --set, --questions, or --noul".to_string(),
        ));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| summonr::Error::Config(format!("cannot read {path}: {error}")))?;
    let examples = summonr::decisions::eval::load(&text)?;
    let endpoint = decisions_endpoint(args)?;
    let room = endpoint
        .context_window
        .map_or(summonr::decisions::CONTEXT_CHARS, |tokens| {
            tokens as usize * 3
        });
    let asked_size = serde_json::to_string(questions).map_or(0, |text| text.len());
    let budget = room.saturating_sub(asked_size);
    let decided = summonr::decisions::eval::run(&endpoint, &examples, questions, budget).await?;
    let (tallies, misses) = summonr::decisions::eval::score(&examples, &decided, floor);
    print!(
        "{}",
        summonr::decisions::eval::report(&tallies, &misses, floor)
    );
    let clean = tallies
        .values()
        .all(|tally| tally.acted == 0 || tally.gated_accuracy() >= 1.0);
    Ok(i32::from(!clean))
}

fn decisions_endpoint(args: &Args) -> Result<summonr::Endpoint> {
    let name = match args.endpoint.as_deref() {
        Some(name) => name.to_string(),
        None => summonr::decisions_endpoint().ok_or_else(|| {
            summonr::Error::Config(
                "no decisions endpoint has a key — set OPENROUTER_API_KEY or TYPESAFE_API_KEY"
                    .to_string(),
            )
        })?,
    };
    resolve_endpoint(Some(&name), args.model.as_deref())
}

async fn run_decide(args: &Args) -> Result<i32> {
    let mut questions: std::collections::BTreeMap<String, summonr::decisions::Question> =
        match args.questions.as_deref() {
            Some(source) => {
                let text = match source.strip_prefix('@') {
                    Some(path) => std::fs::read_to_string(path).map_err(|error| {
                        summonr::Error::Config(format!("cannot read {path}: {error}"))
                    })?,
                    None => source.to_string(),
                };
                serde_json::from_str(&text)?
            }
            None => std::collections::BTreeMap::new(),
        };
    if args.review {
        questions.extend(summonr::decisions::sets::review());
    }
    if let Some(name) = args.set.as_deref() {
        let set = summonr::decisions::sets::by_name(name).ok_or_else(|| {
            summonr::Error::Config(format!(
                "unknown question set `{name}` — try one of: {}",
                summonr::decisions::sets::NAMES.join(", ")
            ))
        })?;
        questions.extend(set);
    }
    for (index, text) in args.noul.iter().enumerate() {
        questions.insert(
            format!("noul{}", index + 1),
            summonr::decisions::Question::Noul {
                instructions: text.clone(),
            },
        );
    }
    let floor = args.gate.unwrap_or(0.7);
    if args.review_diff {
        return run_diff_review(args, &questions, floor).await;
    }
    if let Some(path) = args.eval.as_deref() {
        return run_eval(args, &questions, path, floor).await;
    }
    let state = if args.session.is_some() || args.review {
        let path = match args.session.as_deref() {
            Some(id) => resolve_session(id)?,
            None => session_path(args)?,
        };
        summonr::decisions::digest::digest(&summonr::export_session(&path)?)
    } else if args.prompt.is_empty() {
        let mut input = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
        input
    } else {
        args.prompt.join(" ")
    };
    if state.trim().is_empty() {
        return Err(summonr::Error::Config(
            "give it some state, as arguments, on stdin, or with --session".to_string(),
        ));
    }
    if questions.is_empty() {
        return Err(summonr::Error::Config(
            "ask something: --review, --noul, or --questions".to_string(),
        ));
    }
    let endpoint = decisions_endpoint(args)?;
    let room = endpoint
        .context_window
        .map_or(summonr::decisions::CONTEXT_CHARS, |tokens| {
            tokens as usize * 3
        });
    let asked = serde_json::to_string(&questions).map_or(0, |text| text.len());
    let budget = room.saturating_sub(asked);
    let (decision, windows) =
        summonr::decisions::sweep::sweep(&endpoint, &state, &questions, budget).await?;
    if windows > 1 && !args.json {
        eprintln!("summonr: read the state in {windows} windows and merged the answers");
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "model": decision.model,
                "answers": decision.answers,
                "usage": {
                    "input_tokens": decision.usage.input_tokens,
                    "output_tokens": decision.usage.output_tokens,
                    "cost": decision.usage.cost,
                },
            }))?
        );
    } else {
        print!("{}", summonr::decisions::render(&decision));
    }
    Ok(0)
}

async fn route(args: Args, log: Option<&diagnostics::Log>) -> Result<i32> {
    if args.logs {
        return diagnostics::print_logs(log.map(diagnostics::Log::path));
    }
    if args.doctor {
        return diagnostics::doctor(log.map(diagnostics::Log::path));
    }
    if let Some(words) = &args.room {
        return summonr::room::run(words, &std::env::current_dir()?).await;
    }
    if args.acp {
        return summonr::acp::serve(
            args.endpoint.as_deref(),
            args.model.as_deref(),
            |endpoint, client, cwd| {
                build(&args, endpoint, client, cwd, None, SessionAction::Create)
            },
        )
        .await;
    }
    let adopted = adopt(&args)?;
    if args.login {
        return run_login(&args).await.map(|()| 0);
    }
    if args.logout {
        let Some(name) = args.endpoint.as_deref() else {
            return Err(summonr::Error::Config(
                "logout needs an endpoint name".to_string(),
            ));
        };
        let gone = summonr::forget_key(name)?;
        println!(
            "{}",
            if gone {
                format!("forgot the stored key for {name}")
            } else {
                format!("nothing stored for {name}")
            }
        );
        return Ok(0);
    }
    if args.decide {
        return run_decide(&args).await;
    }
    if args.list {
        return probe_endpoints().await.map(|()| 0);
    }
    if args.list_sessions {
        return print_sessions().map(|()| 0);
    }
    if args.export {
        return export_session_cmd(&args).map(|()| 0);
    }
    if args.profile {
        print!("{}", summonr::profile_session(&session_path(&args)?)?);
        return Ok(0);
    }
    if args.list_workflows {
        return print_workflows().map(|()| 0);
    }
    if args.list_modules {
        println!(
            "{}",
            summonr::modules::Switches::load(&workspace()?)?.table()
        );
        return Ok(0);
    }
    if args.list_worktrees {
        return manage_worktrees(&args).map(|()| 0);
    }
    if args.diff {
        return print_diff(&args).map(|()| 0);
    }
    launch(args, log, &adopted).await
}

async fn launch(mut args: Args, log: Option<&diagnostics::Log>, adopted: &[String]) -> Result<i32> {
    loop {
        match launch_once(&args, log, adopted).await? {
            Launch::Done(code) => return Ok(code),
            Launch::Resume(id) => {
                args.session = Some(id);
                args.resume = false;
                args.fork = false;
                args.name = None;
            }
        }
    }
}

async fn launch_once(
    args: &Args,
    log: Option<&diagnostics::Log>,
    adopted: &[String],
) -> Result<Launch> {
    let cwd = workspace()?;
    let (workspace, tree, action) = isolate(args, &cwd)?;
    let endpoint = if use_tui(args) {
        summonr::resolve_endpoint_unauthed(args.endpoint.as_deref(), args.model.as_deref())?
    } else {
        summonr::resolve_endpoint(args.endpoint.as_deref(), args.model.as_deref())?
    };
    if let Some(name) = args.endpoint.as_deref() {
        summonr::prefs::set_last_endpoint(name);
    }
    if endpoint.source == Some(summonr::secret::Source::Config)
        && !args.json
        && !adopted.contains(&endpoint.name)
    {
        eprintln!(
            "the key for {} is plaintext in config.toml — `summon login {} --key` moves it into the credential store",
            endpoint.name, endpoint.name
        );
    }
    let ui = channels(use_tui(args));
    let provider = provider(&endpoint, ui.0.as_ref(), args.json)?;
    let probe = endpoint.context_window.is_none().then(|| {
        let client = provider.clone();
        let model = endpoint.model.clone();
        tokio::spawn(async move { client.context(&model).await })
    });
    let checks = if args.no_verify {
        None
    } else {
        summonr::check_commands(&workspace)?
    };
    let (check, goal_checks) = route_checks(args.goal.is_some(), checks);
    let mut loop_ = build(args, &endpoint, provider, &workspace, check, action)?;
    let (fallbacks, skipped) = summonr::endpoint_fallbacks(&endpoint, |fallback| {
        self::provider(fallback, ui.0.as_ref(), args.json)
    });
    for note in skipped {
        eprintln!("summonr: {note}");
    }
    loop_.set_fallbacks(fallbacks);
    if let Some(probe) = probe {
        loop_.set_context_probe(probe);
    }
    let workflow = workflow_name(args, &loop_)?
        .map(|name| apply_workflow(&mut loop_, &name, ui.0.as_ref(), args.json))
        .transpose()?;
    let session = loop_.session_id();
    let session_path = loop_.session_path().to_path_buf();
    if let Some(log) = log {
        log.session(&session, loop_.session_path(), &workspace);
    }
    let exit = dispatch(loop_, args, &endpoint, workspace, ui, goal_checks, workflow).await?;
    let code = match exit {
        summonr::tui::Exit::Code(code) => code,
        summonr::tui::Exit::Resume(id) => return Ok(Launch::Resume(id)),
    };
    let Some(tree) = tree else {
        return Ok(Launch::Done(code));
    };
    if code != 0 {
        eprintln!(
            "worktree kept {} ({}) — the run's files are there, uncommitted",
            tree.path.display(),
            tree.branch
        );
        return Ok(Launch::Done(code));
    }
    let live = summonr::live_jobs(&tree.path);
    if !live.is_empty() {
        eprintln!(
            "worktree kept {} ({}): background job{} still running — {}",
            tree.path.display(),
            tree.branch,
            if live.len() == 1 { "" } else { "s" },
            live.join("; ")
        );
        return Ok(Launch::Done(1));
    }
    let touched = Loop::<Client>::touched_at(&session_path);
    match finish_worktree(&tree, &objective(args, &session), &touched) {
        Ok(files) => {
            eprintln!(
                "merged {} into {}: {} file{}{}",
                tree.branch,
                tree.base,
                files.len(),
                if files.len() == 1 { "" } else { "s" },
                files
                    .iter()
                    .map(|file| format!("\n  {file}"))
                    .collect::<String>()
            );
            if args.json {
                let event = summonr::Event::Merged {
                    branch: tree.branch.clone(),
                    base: tree.base.clone(),
                    files,
                };
                if let Ok(line) = serde_json::to_string(&event) {
                    println!("{line}");
                }
            }
            Ok(Launch::Done(code))
        }
        Err(error) => {
            eprintln!(
                "worktree kept {} ({}): {error}",
                tree.path.display(),
                tree.branch
            );
            Ok(Launch::Done(1))
        }
    }
}

fn objective(args: &Args, session: &str) -> String {
    match (args.goal.as_deref(), args.steps.as_deref()) {
        (Some(goal), _) => goal.to_string(),
        (None, Some(plan)) => format!("steps from {plan}"),
        (None, None) => format!("session {session}"),
    }
}

fn route_checks(goal: bool, checks: Option<Vec<String>>) -> (Option<Vec<String>>, Vec<String>) {
    if goal {
        (checks.clone(), checks.unwrap_or_default())
    } else {
        (checks, Vec::new())
    }
}

fn build(
    args: &Args,
    endpoint: &Endpoint,
    provider: Client,
    workspace: &Path,
    check: Option<Vec<String>>,
    action: SessionAction,
) -> Result<Loop<Client>> {
    let switches = summonr::modules::Switches::load(workspace)?;
    let prompt = system_prompt_with(workspace, args.trust, switches.enabled("skills"));
    let resumed = matches!(action, SessionAction::Resume(_) | SessionAction::Fork(_));
    let mut loop_ = match action {
        SessionAction::Create => Loop::new(provider, endpoint.model.clone(), workspace, prompt)?,
        SessionAction::Resume(path) => {
            Loop::resume_path(provider, endpoint.model.clone(), workspace, prompt, &path)?
        }
        SessionAction::Fork(path) => {
            Loop::fork_path(provider, endpoint.model.clone(), workspace, prompt, &path)?
        }
    };
    if resumed && args.goal.is_none() && args.steps.is_none() {
        eprintln!(
            "resuming session {} in {}",
            loop_.session_id(),
            loop_.workspace().display()
        );
    }
    if let Some(name) = &args.name {
        loop_.set_name(name)?;
    }
    loop_.set_context_window(endpoint.context_window);
    loop_.set_compact_tokens(endpoint.compact_tokens);
    loop_.set_effort(args.effort.or(endpoint.effort));
    loop_.set_price(endpoint.price);
    loop_.set_env(endpoint.env.clone());
    loop_.set_modules(&switches);
    configure(loop_, args, check)
}

fn configure(
    mut loop_: Loop<Client>,
    args: &Args,
    check: Option<Vec<String>>,
) -> Result<Loop<Client>> {
    if args.sandbox || args.plan || summonr::sandbox_on()? {
        loop_ = loop_.with_sandbox(!args.network)?;
    } else {
        if !args.network {
            eprintln!("summonr: --no-net needs --sandbox; sockets stay open");
        }
        loop_ = loop_.with_unrestricted_commands();
    }
    for dir in &args.add_dir {
        loop_.allow_path(dir)?;
    }
    if args.plan {
        loop_ = loop_.with_plan();
    }
    if args.strict {
        loop_ = loop_.with_strict();
    }
    if let Some(check) = check {
        loop_ = loop_.with_verify(check);
    }
    if args.max_turns.is_some() && args.goal.is_none() {
        return Err(summonr::Error::Config(
            "--max-turns only applies to --goal".to_string(),
        ));
    }
    let budget = if args.max_tokens.is_some() || args.max_secs.is_some() {
        Some(Budget {
            max_tokens: args.max_tokens,
            max_secs: args.max_secs,
        })
    } else {
        None
    };
    if let Some(budget) = budget {
        loop_ = loop_.with_budget(budget)?;
    }
    Ok(loop_)
}

async fn dispatch(
    loop_: Loop<Client>,
    args: &Args,
    endpoint: &Endpoint,
    workspace: PathBuf,
    ui: Ui,
    goal_checks: Vec<String>,
    workflow: Option<workflow::Workflow>,
) -> Result<summonr::tui::Exit> {
    let model = endpoint.model.as_str();
    let session = loop_.session_id();
    let splash = splash::Splash {
        version: env!("CARGO_PKG_VERSION"),
        model,
        relay: splash::host_of(endpoint.base_url.as_deref()),
        session: &session,
        resumed: args.resume || args.session.is_some() || args.fork,
        history: loop_.history_len(),
        workspace: &workspace,
        plan: args.plan,
        strict: args.strict,
        trust: args.trust,
    };
    if let Some(plan_path) = args.steps.as_deref() {
        splash::line(&splash);
        modes::steps(loop_, args, plan_path)
            .await
            .map(summonr::tui::Exit::Code)
    } else if let Some(objective) = args.goal.as_deref() {
        splash::line(&splash);
        modes::goal(loop_, args, objective, goal_checks)
            .await
            .map(summonr::tui::Exit::Code)
    } else if args.print || !args.prompt.is_empty() {
        splash::line(&splash);
        modes::print(loop_, args)
            .await
            .map(summonr::tui::Exit::Code)
    } else if let Some(ui_rx) = ui.1 {
        let prefs = summonr::prefs::load()?;
        let splash = prefs.splash && !args.no_splash;
        let info = summonr::tui::Info {
            version: env!("CARGO_PKG_VERSION").to_string(),
            model: model.to_string(),
            relay: splash::host_of(endpoint.base_url.as_deref()).map(str::to_string),
            session: loop_.session_id().to_string(),
            endpoint: endpoint.name.clone(),
            prefs,
            splash,
            plan: args.plan,
            strict: args.strict,
            workspace: summonr::tilde(&workspace),
            workflow,
            config: summonr::prefs::path(),
        };
        let ui_tx =
            ui.0.ok_or_else(|| summonr::Error::Config("tui sender missing".to_string()))?;
        if !args.gui {
            let secrets = ui_tx.clone();
            summonr::askpass::start(move |request| {
                drop(secrets.send(summonr::tui::Msg::Secret(request)));
            })?;
        }
        summonr::tui::run(loop_, workspace, info, ui_tx, ui_rx, args.gui)
    } else {
        splash::line(&splash);
        modes::print(loop_, args)
            .await
            .map(summonr::tui::Exit::Code)
    }
}

fn channels(tui: bool) -> Ui {
    if tui {
        let (tx, rx) = unbounded_channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    }
}

fn provider(
    endpoint: &Endpoint,
    ui: Option<&UnboundedSender<summonr::tui::Msg>>,
    json: bool,
) -> Result<Client> {
    let mut provider = Client::connect(endpoint)?;
    if let Some(ui) = ui {
        let text = ui.clone();
        provider = provider.on_delta(move |delta| {
            drop(text.send(summonr::tui::Msg::Event(Event::Delta {
                text: delta.to_string(),
            })));
        });
        let reasoning = ui.clone();
        provider = provider.on_reasoning(move |delta| {
            drop(reasoning.send(summonr::tui::Msg::Event(Event::Reasoning {
                text: delta.to_string(),
            })));
        });
    } else if json {
        provider = provider.on_delta(|text| {
            modes::print_json(&summonr::Event::Delta {
                text: text.to_string(),
            });
        });
        provider = provider.on_reasoning(|text| {
            modes::print_json(&summonr::Event::Reasoning {
                text: text.to_string(),
            });
        });
    } else {
        let thinking = Arc::new(AtomicBool::new(false));
        let text_state = thinking.clone();
        provider = provider.on_delta(move |text| {
            text_state.store(false, Ordering::Relaxed);
            print!("{text}");
            drop(std::io::stdout().flush());
        });
        let reasoning_state = thinking.clone();
        provider = provider.on_reasoning(move |_| {
            if !reasoning_state.swap(true, Ordering::Relaxed) {
                eprintln!("\n… reasoning");
            }
        });
        provider = provider.on_tool(move |_| {
            thinking.store(false, Ordering::Relaxed);
        });
    }
    Ok(provider)
}

fn use_tui(args: &Args) -> bool {
    !args.print
        && args.steps.is_none()
        && args.goal.is_none()
        && args.prompt.is_empty()
        && !args.json
        && (args.gui || (std::io::stdout().is_terminal() && std::io::stdin().is_terminal()))
}

fn candidates(workspace: &Path) -> Vec<PathBuf> {
    let mut paths = vec![workspace.to_path_buf()];
    if let Ok(trees) = list_worktrees(workspace) {
        paths.extend(trees.into_iter().map(|tree| tree.path));
    }
    paths
}

fn load_session(args: &Args, workspace: &Path) -> Result<SessionAction> {
    if (args.steps.is_some() || args.goal.is_some()) && args.fork {
        return Err(summonr::Error::Config(
            "cannot fork an active step or goal session".to_string(),
        ));
    }
    if let Some(id) = args.session.as_deref() {
        let path = resolve_session(id)?;
        return Ok(SessionAction::Resume(path));
    }
    if let Some(plan_path) = args.steps.as_deref() {
        let markdown = std::fs::read_to_string(plan_path)
            .map_err(|error| summonr::Error::Config(format!("cannot read {plan_path}: {error}")))?;
        let roots = candidates(workspace);
        if let Some(info) = list_sessions()?.into_iter().rev().find(|info| {
            roots
                .iter()
                .any(|root| Loop::<Client>::steps_at(&info.path, root, &markdown))
        }) {
            eprintln!("resuming steps from {plan_path} in session {}", info.id);
            return Ok(SessionAction::Resume(info.path));
        }
    }
    if let Some(objective) = args.goal.as_deref() {
        let max_turns = args.max_turns.map_or(0, |turns| turns as usize);
        let roots = candidates(workspace);
        if let Some(info) = list_sessions()?.into_iter().rev().find(|info| {
            roots
                .iter()
                .any(|root| Loop::<Client>::goal_at(&info.path, root, objective, max_turns))
        }) {
            eprintln!(
                "resuming goal in session {} (its turn limit stays as recorded)",
                info.id
            );
            return Ok(SessionAction::Resume(info.path));
        }
    }
    if args.steps.is_some() || args.goal.is_some() {
        let roots = candidates(workspace);
        for info in list_sessions()?.into_iter().rev() {
            if let Some((kind, objective)) = roots
                .iter()
                .find_map(|root| Loop::<Client>::job_at(&info.path, root))
            {
                eprintln!(
                    "note: an interrupted {kind:?} run is still active in session {} — {} — rerun it unchanged or `--session {}` to resume it",
                    info.id,
                    objective.lines().next().unwrap_or_default(),
                    info.id
                );
                break;
            }
        }
    }
    if args.resume || args.fork {
        let path = last_for(workspace)?.ok_or_else(|| {
            summonr::Error::Config("no prior session for this directory".to_string())
        })?;
        if args.fork {
            return Ok(SessionAction::Fork(path));
        }
        return Ok(SessionAction::Resume(path));
    }
    Ok(SessionAction::Create)
}
