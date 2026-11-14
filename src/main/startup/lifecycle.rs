use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use summonr_agent::{
    Client, Delta, Endpoint, Event, Loop, Result, finish_worktree, splash, workflow,
};

use super::commands::workspace;
use super::session::{apply_workflow, build, isolate, set_up, workflow_name};
use crate::entry::cli::Args;
use crate::entry::{diagnostics, modes};

#[path = "lifecycle/selection.rs"]
mod selection;
use selection::{ask_login, selection};

#[cfg(feature = "gui")]
#[path = "lifecycle/desktop.rs"]
mod desktop;
#[cfg(feature = "gui")]
pub(super) use desktop::run as desktop;

#[cfg(not(feature = "gui"))]
pub(super) fn desktop(_args: &Args) -> Result<i32> {
    Err(summonr_agent::Error::Config(
        "this build has no window; install with `cargo install --features gui`".to_string(),
    ))
}

type Ui = (
    Option<summonr_agent::hub::Ui>,
    Option<summonr_agent::hub::Inbox>,
);

pub(super) async fn launch(
    args: &Args,
    log: Option<&diagnostics::Log>,
    adopted: &[String],
) -> Result<i32> {
    let cwd = workspace()?;
    let (workspace, tree, action) = isolate(args, &cwd)?;
    set_up(&cwd, tree.as_ref(), &action, args.json)?;
    let endpoint = selection(args, adopted).await?;
    if use_tui(args) {
        summonr_agent::mark_interactive();
    }
    let ui = channels(use_tui(args));
    ask_login(&ui, &endpoint, args.endpoint.is_some());
    let (loop_, workflow) = assemble(args, &endpoint, &workspace, action, &ui)?;
    if !args.json {
        for check in loop_.verify_commands() {
            eprintln!("summonr: workspace check: {check}");
        }
    }
    let session = loop_.session_id();
    summonr_agent::prune_sessions(Some(&session));
    let session_path = loop_.session_path().to_path_buf();
    if let Some(log) = log {
        log.session(&session, loop_.session_path(), &workspace);
    }
    let code = dispatch(loop_, args, &endpoint, workspace, ui, workflow).await?;
    let Some(tree) = tree else {
        return Ok(code);
    };
    Ok(land(&tree, code, args, &session, &session_path))
}

type Assembled = (Loop<Client>, Option<workflow::Workflow>);

fn factory(args: &Args, endpoint: &Endpoint, cwd: PathBuf) -> summonr_agent::hub::Factory {
    let (args, endpoint) = (args.clone(), endpoint.clone());
    Arc::new(move |open, ui| {
        let mut args = args.clone();
        args.resume = false;
        args.fork = false;
        args.worktree = matches!(open, summonr_agent::hub::Open::Worktree(_));
        args.name = None;
        let (folder, session, topic) = match open {
            summonr_agent::hub::Open::New => (cwd.clone(), None, None),
            summonr_agent::hub::Open::Folder(folder)
            | summonr_agent::hub::Open::Worktree(folder) => (folder, None, None),
            summonr_agent::hub::Open::Resume(id) => (cwd.clone(), Some(id), None),
            summonr_agent::hub::Open::Thread { topic, folder } => (folder, None, Some(topic)),
        };
        args.session = session;
        let (workspace, _, action) = isolate(&args, &folder)?;
        let (mut loop_, workflow) =
            assemble(&args, &endpoint, &workspace, action, &(Some(ui), None))?;
        if let Some(topic) = &topic {
            loop_.join_topic(topic)?;
        }
        Ok((loop_, workflow))
    })
}

fn assemble(
    args: &Args,
    endpoint: &Endpoint,
    workspace: &std::path::Path,
    action: super::session::SessionAction,
    ui: &Ui,
) -> Result<Assembled> {
    let provider = provider(endpoint, ui.0.as_ref(), args.json)?;
    let check = if args.no_verify {
        None
    } else {
        summonr_agent::check_commands(workspace)?
    };
    let spare = endpoint.context_window.is_none().then(|| provider.clone());
    let mut loop_ = build(args, endpoint, provider, workspace, check, action)?;
    let (fallbacks, skipped) = summonr_agent::endpoint_fallbacks(endpoint, |fallback| {
        self::provider(fallback, ui.0.as_ref(), args.json)
    });
    for note in skipped {
        eprintln!("summonr: {note}");
    }
    loop_.set_fallbacks(fallbacks);
    if let Some(client) = spare {
        loop_.watch_driver(client, &endpoint.name, &endpoint.model);
    }
    let workflow = workflow_name(args, &loop_)?
        .map(|name| apply_workflow(&mut loop_, &name, ui.0.as_ref(), args.json))
        .transpose()?;
    Ok((loop_, workflow))
}

fn info(
    args: &Args,
    endpoint: &Endpoint,
    loop_: &Loop<Client>,
    workspace: &std::path::Path,
    workflow: Option<workflow::Workflow>,
) -> Result<summonr_agent::hub::Info> {
    let prefs = summonr_agent::prefs::load()?;
    let splash = prefs.splash.shown() && !args.no_splash;
    Ok(summonr_agent::hub::Info {
        trust: args.trust,
        version: env!("CARGO_PKG_VERSION").to_string(),
        model: endpoint.model.clone(),
        relay: splash::host_of(endpoint.base_url.as_deref()).map(str::to_string),
        session: loop_.session_id().to_string(),
        endpoint: endpoint.name.clone(),
        pick: summonr_agent::endpoint_chosen_by(args.endpoint.as_deref(), args.model.as_deref()),
        prefs,
        splash,
        inline: args.inline,
        strict: args.strict,
        workspace: summonr_agent::tilde(workspace),
        workflow,
        config: summonr_agent::prefs::path(),
        prompt: (!args.prompt.is_empty()).then(|| args.prompt.join(" ")),
    })
}

const LISTED: usize = 3;

fn listed(changes: &[String]) -> String {
    let shown = changes[..changes.len().min(LISTED)].join(", ");
    match changes.len().saturating_sub(LISTED) {
        0 => shown,
        more => format!("{shown} and {more} more"),
    }
}

fn land(
    tree: &summonr_agent::Tree,
    code: i32,
    args: &Args,
    session: &str,
    session_path: &std::path::Path,
) -> i32 {
    if code != 0 {
        let state = match tree.changes() {
            Ok(changes) if changes.is_empty() => "the run made no changes there".to_string(),
            Ok(changes) => format!(
                "the run's files are there, uncommitted: {}",
                listed(&changes)
            ),
            Err(error) => format!("its changes could not be read: {error}"),
        };
        eprintln!(
            "worktree kept {} ({}): {state}",
            tree.path.display(),
            tree.branch
        );
        return code;
    }
    let live = summonr_agent::live_jobs(&tree.path);
    if !live.is_empty() {
        eprintln!(
            "worktree kept {} ({}): background job{} still running; {}",
            tree.path.display(),
            tree.branch,
            if live.len() == 1 { "" } else { "s" },
            live.join("; ")
        );
        return 1;
    }
    let touched = Loop::<Client>::touched_at(session_path);
    match finish_worktree(tree, session, &objective(args, session), &touched) {
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
                modes::print_json(&summonr_agent::Event::Merged {
                    branch: tree.branch.clone(),
                    base: tree.base.clone(),
                    files,
                });
            }
            code
        }
        Err(error) => {
            eprintln!(
                "worktree kept {} ({}): {error}",
                tree.path.display(),
                tree.branch
            );
            1
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

async fn dispatch(
    loop_: Loop<Client>,
    args: &Args,
    endpoint: &Endpoint,
    workspace: PathBuf,
    ui: Ui,
    workflow: Option<workflow::Workflow>,
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
        strict: args.strict,
        trust: args.trust,
    };
    if let Some(plan_path) = args.steps.as_deref() {
        splash.line();
        modes::steps(loop_, args, plan_path).await
    } else if let Some(objective) = args.goal.as_deref() {
        splash.line();
        modes::goal(loop_, args, objective).await
    } else if let Some(ui_rx) = ui.1 {
        let info = info(args, endpoint, &loop_, &workspace, workflow)?;
        let ui_tx =
            ui.0.ok_or_else(|| summonr_agent::Error::Config("tui sender missing".to_string()))?;
        let secrets = ui_tx.clone();
        summonr_agent::askpass::start(move |request| {
            secrets.secret(request);
        })?;
        let opener = factory(args, endpoint, super::commands::workspace()?);
        let stderr = crate::entry::diagnostics::park_stderr();
        let result = summonr_ui::run(loop_, workspace, info, ui_tx, ui_rx, opener);
        crate::entry::diagnostics::restore_stderr(stderr);
        result
    } else {
        splash.line();
        modes::print(loop_, args).await
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stream {
    Idle,
    Reasoning,
    Answer,
}

fn channels(tui: bool) -> Ui {
    if tui {
        let (ui, inbox) = summonr_agent::hub::channel();
        (Some(ui), Some(inbox))
    } else {
        (None, None)
    }
}

pub(super) fn provider(
    endpoint: &Endpoint,
    ui: Option<&summonr_agent::hub::Ui>,
    json: bool,
) -> Result<Client> {
    if let Some(ui) = ui {
        return ui.client(endpoint);
    }
    let provider = Client::connect(endpoint)?;
    if json {
        return Ok(provider.on_stream(|delta| match delta {
            Delta::Text(text) => modes::print_json(&Event::Delta { text: text.clone() }),
            Delta::Thinking(text) => modes::print_json(&Event::Reasoning { text: text.clone() }),
            Delta::Call(_) | Delta::Progress => {}
        }));
    }
    let stream = Mutex::new(Stream::Idle);
    Ok(provider.on_stream(move |delta| {
        let mut state = stream.lock().unwrap_or_else(PoisonError::into_inner);
        match delta {
            Delta::Text(text) => {
                *state = Stream::Answer;
                print!("{text}");
                drop(std::io::stdout().flush());
            }
            Delta::Thinking(_) if *state == Stream::Idle => {
                *state = Stream::Reasoning;
                eprintln!("\n… reasoning");
            }
            Delta::Call(_) => *state = Stream::Idle,
            Delta::Thinking(_) | Delta::Progress => {}
        }
    }))
}

pub(super) fn use_tui(args: &Args) -> bool {
    !args.print
        && args.steps.is_none()
        && args.goal.is_none()
        && !args.json
        && (args.gui || (std::io::stdout().is_terminal() && std::io::stdin().is_terminal()))
}
