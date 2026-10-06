use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use summonr::{Client, Endpoint, Event, Loop, Result, finish_worktree, splash, workflow};

use super::commands::workspace;
use super::session::{apply_workflow, build, isolate, route_checks, workflow_name};
use crate::cli::Args;
use crate::{diagnostics, modes};

type Ui = (Option<summonr::tui::Ui>, Option<summonr::tui::Inbox>);

pub(super) async fn launch(
    args: &Args,
    log: Option<&diagnostics::Log>,
    adopted: &[String],
) -> Result<i32> {
    let cwd = workspace()?;
    let (workspace, tree, action) = isolate(args, &cwd)?;
    let endpoint = seat(args, adopted)?;
    let ui = channels(use_tui(args));
    ask_login(&ui, &endpoint, args.endpoint.is_some());
    let (loop_, goal_checks, workflow) = assemble(args, &endpoint, &workspace, action, &ui)?;
    let session = loop_.session_id();
    summonr::prune_sessions(Some(&session));
    let session_path = loop_.session_path().to_path_buf();
    if let Some(log) = log {
        log.session(&session, loop_.session_path(), &workspace);
    }
    let code = dispatch(loop_, args, &endpoint, workspace, ui, goal_checks, workflow).await?;
    let Some(tree) = tree else {
        return Ok(code);
    };
    Ok(land(&tree, code, args, &session, &session_path))
}

type Assembled = (Loop<Client>, Vec<String>, Option<workflow::Workflow>);

fn factory(args: &Args, endpoint: &Endpoint, cwd: PathBuf) -> summonr::tui::hub::Factory {
    let (args, endpoint) = (args.clone(), endpoint.clone());
    Arc::new(move |open, ui| {
        let mut args = args.clone();
        args.resume = false;
        args.fork = false;
        args.worktree = false;
        args.name = None;
        args.session = match open {
            summonr::tui::hub::Open::New => None,
            summonr::tui::hub::Open::Resume(id) => Some(id),
        };
        let (workspace, _, action) = isolate(&args, &cwd)?;
        let (loop_, _, workflow) =
            assemble(&args, &endpoint, &workspace, action, &(Some(ui), None))?;
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
    let probe = endpoint.context_window.is_none().then(|| {
        let client = provider.clone();
        let name = endpoint.name.clone();
        let model = endpoint.model.clone();
        tokio::spawn(async move { client.context(&name, &model).await })
    });
    let checks = if args.no_verify {
        None
    } else {
        summonr::check_commands(workspace)?
    };
    let (check, goal_checks) = route_checks(args.goal.is_some(), checks);
    let mut loop_ = build(args, endpoint, provider, workspace, check, action)?;
    let (fallbacks, skipped) = summonr::endpoint_fallbacks(endpoint, |fallback| {
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
    Ok((loop_, goal_checks, workflow))
}

fn seat(args: &Args, adopted: &[String]) -> Result<Endpoint> {
    let need = if use_tui(args) {
        summonr::Need::Model
    } else {
        summonr::Need::Seat
    };
    let endpoint =
        summonr::resolve_endpoint(args.endpoint.as_deref(), args.model.as_deref(), need)?;
    if let Some(name) = args.endpoint.as_deref() {
        summonr::prefs::set_last_endpoint(name);
    }
    if endpoint.source == Some(summonr::secret::Source::Config)
        && !args.json
        && !adopted.contains(&endpoint.name)
    {
        eprintln!(
            "the key for {} is plaintext in config.toml; `summonr login {} --key` moves it into the credential store",
            endpoint.name, endpoint.name
        );
    }
    Ok(endpoint)
}

fn ask_login(ui: &Ui, endpoint: &Endpoint, named: bool) {
    if let Some(ui) = &ui.0
        && endpoint.api_key.is_none()
        && endpoint.oauth.is_none()
    {
        let (ui, seat) = (ui.clone(), endpoint.clone());
        drop(tokio::spawn(async move {
            let note = if !seat.keyless() && named {
                format!(
                    "{} has no key yet. Paste its key, or pick another provider.",
                    seat.name
                )
            } else if !seat.keyless() {
                "No provider is set up yet. Pick one and paste its key.".to_string()
            } else {
                match Client::discover(&seat).await {
                    Err(error) if Client::state(&error) == summonr::EndpointState::KeyRejected => {
                        format!(
                            "{} rejected the request. Paste its key, or pick another provider.",
                            seat.name
                        )
                    }
                    Ok(_) | Err(_) => return,
                }
            };
            ui.open_login(note);
        }));
    }
}

fn land(
    tree: &summonr::Tree,
    code: i32,
    args: &Args,
    session: &str,
    session_path: &std::path::Path,
) -> i32 {
    if code != 0 {
        eprintln!(
            "worktree kept {} ({}): the run's files are there, uncommitted",
            tree.path.display(),
            tree.branch
        );
        return code;
    }
    let live = summonr::live_jobs(&tree.path);
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
    match finish_worktree(tree, &objective(args, session), &touched) {
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
    goal_checks: Vec<String>,
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
        let prefs = summonr::prefs::load()?;
        let splash = prefs.splash && !args.no_splash;
        let info = summonr::tui::Info {
            version: env!("CARGO_PKG_VERSION").to_string(),
            model: model.to_string(),
            relay: splash::host_of(endpoint.base_url.as_deref()).map(str::to_string),
            session: loop_.session_id().to_string(),
            endpoint: endpoint.name.clone(),
            pick: summonr::endpoint_chosen_by(args.endpoint.as_deref(), args.model.as_deref()),
            prefs,
            splash,
            inline: args.inline,
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
                secrets.secret(request);
            })?;
        }
        let opener = factory(args, endpoint, super::commands::workspace()?);
        summonr::tui::run(loop_, workspace, info, ui_tx, ui_rx, args.gui, opener)
    } else {
        splash::line(&splash);
        modes::print(loop_, args).await
    }
}

fn channels(tui: bool) -> Ui {
    if tui {
        let (ui, inbox) = summonr::tui::channel();
        (Some(ui), Some(inbox))
    } else {
        (None, None)
    }
}

pub(super) fn provider(
    endpoint: &Endpoint,
    ui: Option<&summonr::tui::Ui>,
    json: bool,
) -> Result<Client> {
    let mut provider = Client::connect(endpoint)?;
    if let Some(ui) = ui {
        let text = ui.clone();
        provider = provider.on_delta(move |delta| {
            text.event(Event::Delta {
                text: delta.to_string(),
            });
        });
        let reasoning = ui.clone();
        provider = provider.on_reasoning(move |delta| {
            reasoning.event(Event::Reasoning {
                text: delta.to_string(),
            });
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
