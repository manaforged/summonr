use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use summoner::{
    Budget, Client, Endpoint, Loop, Result, add_worktree, finish_worktree, in_git, last_for,
    list_sessions, list_worktrees, resolve_session, splash, sweep_worktrees, system_prompt_with,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::cli::{self, Args};
use super::{diagnostics, modes};

type Ui = (
    Option<UnboundedSender<summoner::tui::Msg>>,
    Option<UnboundedReceiver<summoner::tui::Msg>>,
);
enum SessionAction {
    Create,
    Resume(PathBuf),
    Fork(PathBuf),
}

/// `summon endpoints`: every name, and for the ones that resolve, whether
/// the host answers `GET /models` with this seat's bearer.
async fn probe_endpoints() -> Result<()> {
    for name in summoner::endpoint_names()? {
        let endpoint = match summoner::resolve_endpoint(Some(&name), None) {
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
        println!("{name:<14} {:<22} {host:<34} {status}", endpoint.model);
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
            "{}\t{}\t{}",
            info.id,
            info.cwd.as_deref().unwrap_or("-"),
            info.preview
        );
    }
    Ok(())
}

fn workspace() -> Result<PathBuf> {
    std::env::current_dir().map_err(|error| summoner::Error::Config(error.to_string()))
}

fn isolate(args: &Args, cwd: &Path) -> Result<(PathBuf, Option<summoner::Tree>)> {
    if args.here {
        return Ok((cwd.to_path_buf(), None));
    }
    match load_session(args, cwd)? {
        SessionAction::Resume(path) | SessionAction::Fork(path) => {
            let workspace = journal_workspace(&path).unwrap_or_else(|| cwd.to_path_buf());
            let tree = list_worktrees(&workspace)
                .ok()
                .and_then(|trees| trees.into_iter().find(|tree| tree.path == workspace));
            Ok((workspace, tree))
        }
        SessionAction::Create if wants_tree(args) => {
            if !in_git(cwd) {
                if !args.json {
                    eprintln!("not a git repository; running in {}", cwd.display());
                }
                return Ok((cwd.to_path_buf(), None));
            }
            let tree = add_worktree(cwd)?;
            if !args.json {
                println!("worktree {} ({})", tree.path.display(), tree.branch);
            }
            Ok((tree.path.clone(), Some(tree)))
        }
        SessionAction::Create => Ok((cwd.to_path_buf(), None)),
    }
}

fn wants_tree(args: &Args) -> bool {
    args.worktree || args.goal.is_some() || args.steps.is_some()
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
    let trees = list_worktrees(&cwd)?;
    if trees.is_empty() {
        println!("no worktrees");
        return Ok(());
    }
    for tree in trees {
        println!("{}\t{}", tree.branch, tree.path.display());
    }
    Ok(())
}

async fn run_login(args: &Args) -> Result<()> {
    let endpoint = summoner::resolve_endpoint(args.endpoint.as_deref(), args.model.as_deref())?;
    println!("login {}", endpoint.name);
    summoner::Client::login(&endpoint, |line| println!("{line}")).await?;
    println!("ok");
    Ok(())
}

pub(super) async fn run() -> Result<i32> {
    let log = diagnostics::Log::start();
    let result = match cli::parse() {
        Ok(args) => route(args, log.as_ref()).await,
        Err(error) => Err(error),
    };
    let (code, error) = match &result {
        Ok(code) => (*code, None),
        Err(error) => (1, Some(error.to_string())),
    };
    if let Some(log) = log {
        log.finish(code, error.as_deref());
    }
    result
}

async fn route(args: Args, log: Option<&diagnostics::Log>) -> Result<i32> {
    if args.login {
        return run_login(&args).await.map(|()| 0);
    }
    if args.list {
        return probe_endpoints().await.map(|()| 0);
    }
    if args.list_sessions {
        return print_sessions().map(|()| 0);
    }
    if args.list_worktrees {
        return manage_worktrees(&args).map(|()| 0);
    }
    launch(args, log).await
}

async fn launch(args: Args, log: Option<&diagnostics::Log>) -> Result<i32> {
    let cwd = workspace()?;
    let (workspace, tree) = isolate(&args, &cwd)?;
    let endpoint = summoner::resolve_endpoint(args.endpoint.as_deref(), args.model.as_deref())?;
    let ui = channels(use_tui(&args));
    let provider = provider(&endpoint, ui.0.as_ref(), args.json)?;
    let checks = if args.no_verify {
        None
    } else {
        summoner::check_commands(&workspace)?
    };
    let (check, goal_checks) = route_checks(args.goal.is_some(), checks);
    let loop_ = build(&args, &endpoint, provider, &workspace, check)?;
    if let Some(log) = log {
        let session = loop_.session_id();
        log.session(&session, loop_.session_path(), &workspace);
    }
    let code = dispatch(loop_, &args, &endpoint, workspace, ui, goal_checks).await?;
    if code == 0
        && let Some(tree) = tree
        && let Err(error) = finish_worktree(&tree)
        && !args.json
    {
        eprintln!("worktree kept {}: {error}", tree.path.display());
    }
    Ok(code)
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
) -> Result<Loop<Client>> {
    let prompt = system_prompt_with(workspace, args.trust);
    let action = load_session(args, workspace)?;
    let mut loop_ = match action {
        SessionAction::Create => Loop::new(provider, endpoint.model.clone(), workspace, prompt)?,
        SessionAction::Resume(path) => {
            Loop::resume_path(provider, endpoint.model.clone(), workspace, prompt, &path)?
        }
        SessionAction::Fork(path) => {
            Loop::fork_path(provider, endpoint.model.clone(), workspace, prompt, &path)?
        }
    };
    loop_.set_context_window(endpoint.context_window);
    loop_.set_env(endpoint.env.clone());
    configure(loop_, args, check)
}

fn configure(
    mut loop_: Loop<Client>,
    args: &Args,
    check: Option<Vec<String>>,
) -> Result<Loop<Client>> {
    if args.sandbox {
        loop_ = loop_.with_sandbox(args.no_net)?;
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
    if let Some(rounds) = args.max_rounds {
        loop_ = loop_.with_max_rounds(rounds);
    }
    if args.max_turns.is_some() && args.goal.is_none() {
        return Err(summoner::Error::Config(
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
) -> Result<i32> {
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
        modes::steps(loop_, args, plan_path).await
    } else if let Some(objective) = args.goal.as_deref() {
        splash::line(&splash);
        modes::goal(loop_, args, objective, goal_checks).await
    } else if args.print || !args.prompt.is_empty() {
        splash::line(&splash);
        modes::print(loop_, args).await
    } else if let Some(ui_rx) = ui.1 {
        let mut prefs = summoner::prefs::load()?;
        if args.no_splash {
            prefs.splash = false;
        }
        let info = summoner::tui::Info {
            version: env!("CARGO_PKG_VERSION").to_string(),
            model: model.to_string(),
            relay: splash::host_of(endpoint.base_url.as_deref()).map(str::to_string),
            session: loop_.session_id().to_string(),
            endpoint: endpoint.name.clone(),
            prefs,
            plan: args.plan,
            strict: args.strict,
            workspace: splash::short_path(&workspace),
        };
        let ui_tx =
            ui.0.ok_or_else(|| summoner::Error::Config("tui sender missing".to_string()))?;
        Ok(summoner::tui::run(loop_, workspace, info, ui_tx, ui_rx)?)
    } else {
        let full = !args.no_splash && !args.json;
        splash::interactive(&splash, full);
        modes::interactive(loop_, &workspace, args.json).await
    }
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;

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
    ui: Option<&UnboundedSender<summoner::tui::Msg>>,
    json: bool,
) -> Result<Client> {
    let mut provider = Client::connect(endpoint)?;
    if let Some(ui) = ui {
        let text = ui.clone();
        provider = provider.on_delta(move |delta| {
            drop(text.send(summoner::tui::Msg::Delta(delta.to_string())));
        });
        let reasoning = ui.clone();
        provider = provider.on_reasoning(move |delta| {
            drop(reasoning.send(summoner::tui::Msg::Thinking(delta.to_string())));
        });
    } else if !json {
        provider = provider.on_delta(|text| {
            print!("{text}");
            drop(std::io::stdout().flush());
        });
    }
    Ok(provider)
}

/// Full-screen interface only for a real interactive session.
fn use_tui(args: &Args) -> bool {
    !args.print
        && args.steps.is_none()
        && args.goal.is_none()
        && args.prompt.is_empty()
        && !args.json
        && !args.no_tui
        && std::io::stdout().is_terminal()
        && std::io::stdin().is_terminal()
}

fn load_session(args: &Args, workspace: &Path) -> Result<SessionAction> {
    if (args.steps.is_some() || args.goal.is_some()) && args.fork {
        return Err(summoner::Error::Config(
            "cannot fork an active step or goal session".to_string(),
        ));
    }
    if let Some(id) = args.session.as_deref() {
        let path = resolve_session(id)?;
        return Ok(SessionAction::Resume(path));
    }
    if let Some(plan_path) = args.steps.as_deref() {
        let markdown = std::fs::read_to_string(plan_path).map_err(|error| {
            summoner::Error::Config(format!("cannot read {plan_path}: {error}"))
        })?;
        if let Some(path) = list_sessions()?.into_iter().rev().find_map(|info| {
            Loop::<Client>::steps_at(&info.path, workspace, &markdown).then_some(info.path)
        }) {
            return Ok(SessionAction::Resume(path));
        }
    }
    if let Some(objective) = args.goal.as_deref() {
        let max_turns = args
            .max_turns
            .map_or(summoner::DEFAULT_MAX_TURNS, |turns| turns as usize);
        if let Some(path) = list_sessions()?.into_iter().rev().find_map(|info| {
            Loop::<Client>::goal_at(&info.path, workspace, objective, max_turns)
                .then_some(info.path)
        }) {
            return Ok(SessionAction::Resume(path));
        }
    }
    if args.resume || args.fork {
        let path = last_for(workspace)?.ok_or_else(|| {
            summoner::Error::Config("no prior session for this directory".to_string())
        })?;
        if args.fork {
            return Ok(SessionAction::Fork(path));
        }
        return Ok(SessionAction::Resume(path));
    }
    Ok(SessionAction::Create)
}
