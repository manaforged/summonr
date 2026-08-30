use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use summoner::workflow;
use summoner::{
    Budget, Client, Endpoint, Loop, Result, add_worktree, finish_worktree, in_git, last_for,
    list_sessions, list_worktrees, resolve_endpoint, resolve_session, splash, sweep_worktrees,
    system_prompt_with,
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
        println!(
            "{name:<14} {:<22} {host:<34} {status} · key: {}",
            endpoint.model,
            endpoint
                .source
                .map_or("none", summoner::secret::Source::label)
        );
    }
    Ok(())
}

fn export_session_cmd(args: &Args) -> Result<()> {
    let path = match args.prompt.first() {
        Some(id) => resolve_session(id)?,
        None => last_for(&workspace()?)?.ok_or_else(|| {
            summoner::Error::Config("no prior session for this directory".to_string())
        })?,
    };
    print!("{}", summoner::export_session(&path)?);
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
            summoner::short_session(&info.id),
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

fn apply_workflow(loop_: &mut Loop<Client>, name: &str, json: bool) -> Result<()> {
    let workflow = workflow::load(name)?;
    for (role, seat) in &workflow.seats {
        let endpoint = resolve_endpoint(Some(&seat.endpoint), seat.model.as_deref())
            .map_err(|error| summoner::Error::Config(format!("{name}: {role} seat: {error}")))?;
        let client = provider(&endpoint, None, json)?;
        loop_.set_seat(
            *role,
            client,
            endpoint.model,
            endpoint.effort,
            endpoint.price,
        );
    }
    loop_.set_workflow(Some(name))?;
    Ok(())
}

fn workspace() -> Result<PathBuf> {
    std::env::current_dir().map_err(|error| summoner::Error::Config(error.to_string()))
}

fn isolate(args: &Args, cwd: &Path) -> Result<(PathBuf, Option<summoner::Tree>, SessionAction)> {
    let action = load_session(args, cwd)?;
    match &action {
        SessionAction::Resume(path) | SessionAction::Fork(path) => {
            let workspace = journal_workspace(path).unwrap_or_else(|| cwd.to_path_buf());
            let tree = list_worktrees(&workspace)
                .ok()
                .and_then(|trees| trees.into_iter().find(|tree| tree.path == workspace));
            Ok((workspace, tree, action))
        }
        SessionAction::Create if args.here => Ok((cwd.to_path_buf(), None, action)),
        SessionAction::Create if wants_tree(args) => {
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
    let trees = summoner::worktree_status(&cwd)?;
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
            row.session.as_deref().map_or("-", summoner::short_session),
            row.tree.path.display()
        );
    }
    Ok(())
}

fn turn_paths(cwd: &Path, turn: &str) -> Result<Vec<String>> {
    let journal = summoner::last_for(cwd)?
        .ok_or_else(|| summoner::Error::Config(format!("no session for {}", cwd.display())))?;
    let turns = summoner::diff::turns(&journal)?;
    let index = if turn == "last" {
        turns.len().checked_sub(1)
    } else {
        turn.parse::<usize>().ok().and_then(|n| n.checked_sub(1))
    };
    index
        .and_then(|index| turns.get(index).cloned())
        .ok_or_else(|| {
            summoner::Error::Config(format!(
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
        let mut diff = summoner::diff::stats(&cwd)?;
        diff.files.retain(|stat| keep(&stat.path));
        println!("{}", serde_json::to_string(&diff)?);
        return Ok(());
    }
    if args.stat {
        let mut diff = summoner::diff::stats(&cwd)?;
        diff.files.retain(|stat| keep(&stat.path));
        for stat in &diff.files {
            let new = if stat.new { "  new" } else { "" };
            println!("+{:<5} −{:<5} {}{new}", stat.added, stat.removed, stat.path);
        }
        println!("{}", diff.summary());
        return Ok(());
    }
    match only {
        None => print!("{}", summoner::diff::patch(&cwd, path)?),
        Some(only) => {
            for file in only
                .iter()
                .filter(|file| path.is_none_or(|path| path == *file))
            {
                print!("{}", summoner::diff::patch(&cwd, Some(file))?);
            }
        }
    }
    Ok(())
}

async fn run_login(args: &Args) -> Result<()> {
    if args.endpoint.is_none() {
        return Err(summoner::Error::Config(
            "login needs an endpoint name, for example `summon login copilot`; `summon endpoints` lists them".to_string(),
        ));
    }
    if args.key {
        let name = args.endpoint.as_deref().unwrap_or_default();
        let mut key = String::new();
        std::io::stdin().read_line(&mut key)?;
        let source = summoner::save_key(name, key.trim())?;
        println!("key for {name} stored in {}", source.label());
        return Ok(());
    }
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
    if args.logout {
        let Some(name) = args.endpoint.as_deref() else {
            return Err(summoner::Error::Config(
                "logout needs an endpoint name".to_string(),
            ));
        };
        let gone = summoner::forget_key(name)?;
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
    if args.list {
        return probe_endpoints().await.map(|()| 0);
    }
    if args.list_sessions {
        return print_sessions().map(|()| 0);
    }
    if args.export {
        return export_session_cmd(&args).map(|()| 0);
    }
    if args.list_workflows {
        return print_workflows().map(|()| 0);
    }
    if args.list_worktrees {
        return manage_worktrees(&args).map(|()| 0);
    }
    if args.diff {
        return print_diff(&args).map(|()| 0);
    }
    launch(args, log).await
}

async fn launch(args: Args, log: Option<&diagnostics::Log>) -> Result<i32> {
    let cwd = workspace()?;
    let (workspace, tree, action) = isolate(&args, &cwd)?;
    let endpoint = if use_tui(&args) {
        summoner::resolve_endpoint_unauthed(args.endpoint.as_deref(), args.model.as_deref())?
    } else {
        summoner::resolve_endpoint(args.endpoint.as_deref(), args.model.as_deref())?
    };
    if let Some(name) = args.endpoint.as_deref() {
        summoner::prefs::set_last_endpoint(name);
    }
    if endpoint.source == Some(summoner::secret::Source::Config) && !args.json {
        eprintln!(
            "the key for {} is plaintext in config.toml — `summon login {} --key` moves it to the keychain",
            endpoint.name, endpoint.name
        );
    }
    let ui = channels(use_tui(&args));
    let provider = provider(&endpoint, ui.0.as_ref(), args.json)?;
    let checks = if args.no_verify {
        None
    } else {
        summoner::check_commands(&workspace)?
    };
    let (check, goal_checks) = route_checks(args.goal.is_some(), checks);
    let mut loop_ = build(&args, &endpoint, provider, &workspace, check, action)?;
    let workflow = workflow_name(&args, &loop_)?;
    if !use_tui(&args)
        && let Some(name) = &workflow
    {
        apply_workflow(&mut loop_, name, args.json)?;
    }
    let session = loop_.session_id();
    let session_path = loop_.session_path().to_path_buf();
    if let Some(log) = log {
        log.session(&session, loop_.session_path(), &workspace);
    }
    let code = dispatch(
        loop_,
        &args,
        &endpoint,
        workspace,
        ui,
        goal_checks,
        workflow,
    )
    .await?;
    let Some(tree) = tree else {
        return Ok(code);
    };
    if code != 0 {
        eprintln!(
            "worktree kept {} ({}) — the run's files are there, uncommitted",
            tree.path.display(),
            tree.branch
        );
        return Ok(code);
    }
    let live = summoner::live_jobs(&tree.path);
    if !live.is_empty() {
        eprintln!(
            "worktree kept {} ({}): background job{} still running — {}",
            tree.path.display(),
            tree.branch,
            if live.len() == 1 { "" } else { "s" },
            live.join("; ")
        );
        return Ok(1);
    }
    let touched = Loop::<Client>::touched_at(&session_path);
    match finish_worktree(&tree, &objective(&args, &session), &touched) {
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
                let event = summoner::Event::Merged {
                    branch: tree.branch.clone(),
                    base: tree.base.clone(),
                    files,
                };
                if let Ok(line) = serde_json::to_string(&event) {
                    println!("{line}");
                }
            }
            Ok(code)
        }
        Err(error) => {
            eprintln!(
                "worktree kept {} ({}): {error}",
                tree.path.display(),
                tree.branch
            );
            Ok(1)
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
    let prompt = system_prompt_with(workspace, args.trust);
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
    loop_.set_effort(args.effort.or(endpoint.effort));
    loop_.set_price(endpoint.price);
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
    workflow: Option<String>,
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
            workflow,
            config: summoner::prefs::path(),
        };
        let ui_tx =
            ui.0.ok_or_else(|| summoner::Error::Config("tui sender missing".to_string()))?;
        Ok(summoner::tui::run(
            loop_, workspace, info, ui_tx, ui_rx, args.gui,
        )?)
    } else {
        splash::line(&splash);
        modes::print(loop_, args).await
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
        let max_turns = args
            .max_turns
            .map_or(summoner::DEFAULT_MAX_TURNS, |turns| turns as usize);
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
            summoner::Error::Config("no prior session for this directory".to_string())
        })?;
        if args.fork {
            return Ok(SessionAction::Fork(path));
        }
        return Ok(SessionAction::Resume(path));
    }
    Ok(SessionAction::Create)
}
