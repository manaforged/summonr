use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use summonr::{Client, Endpoint, Event, Loop, Result, finish_worktree, splash, workflow};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::commands::workspace;
use super::session::{apply_workflow, build, isolate, route_checks, workflow_name};
use crate::cli::Args;
use crate::{diagnostics, modes};

type Ui = (
    Option<UnboundedSender<summonr::tui::Msg>>,
    Option<UnboundedReceiver<summonr::tui::Msg>>,
);

enum Launch {
    Done(i32),
    Resume(String),
}

pub(super) async fn launch(
    mut args: Args,
    log: Option<&diagnostics::Log>,
    adopted: &[String],
) -> Result<i32> {
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

pub(super) fn provider(
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
