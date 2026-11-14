#[path = "startup/classify.rs"]
mod classify;
#[path = "startup/commands.rs"]
mod commands;
#[path = "startup/diff.rs"]
mod diff;
#[path = "startup/endpoints.rs"]
mod endpoints;
#[path = "startup/lifecycle.rs"]
mod lifecycle;
#[path = "startup/login.rs"]
mod login;
#[path = "startup/session.rs"]
mod session;
#[path = "startup/tool.rs"]
mod tool;

use summonr_agent::Result;

use super::cli::{Args, Verb};
use super::diagnostics;
use classify::run_classify;
use commands::{
    adopt, export_session_cmd, grep_sessions, manage_worktrees, mcp, print_children,
    print_sessions, print_workflows, session_path, workspace,
};
use diff::print_diff;
use lifecycle::launch;
use login::run_login;
use session::{SessionAction, build};

pub(super) async fn run(args: Args) -> Result<i32> {
    let log = if matches!(
        args.verb,
        Some(Verb::Mail(_) | Verb::Cron(_) | Verb::Gateway(_))
    ) {
        None
    } else {
        diagnostics::Log::start()
    };
    let events = args.json
        && (args.print || args.goal.is_some() || args.steps.is_some() || !args.prompt.is_empty());
    let (result, abort) = match super::until_signal(route(args, log.as_ref())).await {
        Ok(Ok(code)) => (Ok(code), None),
        Ok(Err(error)) => {
            let reason = error.to_string();
            (Err(error), Some((reason, summonr_agent::AbortCode::Error)))
        }
        Err(code) => (
            Ok(code),
            Some((signal(code), summonr_agent::AbortCode::Interrupted)),
        ),
    };
    if events && let Some((reason, code)) = &abort {
        super::modes::print_json(&summonr_agent::Event::Aborted {
            reason: reason.clone(),
            code: *code,
        });
    }
    let (code, error) = match &result {
        Ok(code) => (*code, None),
        Err(error) => (1, Some(error.to_string())),
    };
    if let Some(log) = log {
        log.finish(code, error.as_deref());
    }
    result
}

fn signal(code: i32) -> String {
    match code {
        143 => "terminated by SIGTERM",
        129 => "the terminal hung up",
        _ => "interrupted by Ctrl-C",
    }
    .to_string()
}

async fn route(args: Args, log: Option<&diagnostics::Log>) -> Result<i32> {
    if args.gui && args.verb.is_none() {
        return lifecycle::desktop(&args);
    }
    summonr_agent::check_config()?;
    let adopted = adopt(&args)?;
    match args.verb.as_ref() {
        Some(verb) => run_verb(&args, log, verb).await,
        None => launch(&args, log, &adopted).await,
    }
}

async fn run_verb(args: &Args, log: Option<&diagnostics::Log>, verb: &Verb) -> Result<i32> {
    let in_cwd = |run: &dyn Fn(&std::path::Path) -> Result<i32>| {
        std::env::current_dir()
            .map_err(Into::into)
            .and_then(|cwd| run(&cwd))
    };
    let done = |result: Result<()>| result.map(|()| 0);
    let path = log.map(diagnostics::Log::path);
    match verb {
        Verb::Doctor if args.logs => diagnostics::print_logs(path),
        Verb::Doctor if args.report => diagnostics::report(path),
        Verb::Doctor => diagnostics::doctor(path, args.trust),
        Verb::Mail(command) => {
            in_cwd(&|cwd| summonr_agent::mail::cli::run(command, args.json, cwd))
        }
        Verb::Cron(command) => {
            in_cwd(&|cwd| summonr_agent::cron::cli::run(command, args.json, cwd))
        }
        Verb::Gateway(command) => summonr_agent::gateway::cli::run(command, &login::read_secret),
        Verb::Panel(command) => {
            let cwd = std::env::current_dir()?;
            summonr_agent::panel::cli::run(command, &cwd, args.trust).await
        }
        Verb::Tool { name, arguments } => tool::run(name, arguments).await,
        Verb::Acp if args.schema => {
            println!("{:#}", summonr_agent::acp::schema());
            Ok(0)
        }
        Verb::Acp => acp(args).await,
        Verb::Login => done(login(args).await),
        Verb::Logout => logout(args),
        Verb::Classify => run_classify(args).await,
        Verb::EndpointList => done(endpoints::list(args.all, args.live).await),
        Verb::EndpointUsage => done(endpoints::usage(args.json).await),
        Verb::Mcp(command) => done(mcp(command, args.json).await),
        Verb::SessionList => done(match args.grep.as_deref() {
            Some(pattern) => grep_sessions(pattern),
            None => print_sessions(args.json),
        }),
        Verb::Export => done(export_session_cmd(args)),
        Verb::Profile => done(profile(args)),
        Verb::Workflows => done(print_workflows()),
        Verb::Modules => done(
            workspace()
                .and_then(|root| summonr_agent::modules::load(&root))
                .map(|switches| println!("{}", switches.table())),
        ),
        Verb::PluginRemove => uninstall(args),
        Verb::Marketplace(command) => done(
            summonr_agent::plugins::market::run(command, args.json)
                .await
                .map(|text| println!("{text}")),
        ),
        Verb::Update => update().await,
        Verb::PluginInstall => add(args).await,
        Verb::Worktrees => done(manage_worktrees(args)),
        Verb::Diff => done(print_diff(args)),
        Verb::Children => done(print_children(args)),
    }
}

async fn acp(args: &Args) -> Result<i32> {
    summonr_agent::acp::serve(
        args.endpoint.as_deref(),
        args.model.as_deref(),
        |endpoint, client, cwd, resume| {
            let action = resume.map_or(SessionAction::Create, SessionAction::Resume);
            build(args, endpoint, client, cwd, None, action)
                .map(summonr_agent::Loop::with_interactive)
        },
    )
    .await
}

async fn login(args: &Args) -> Result<()> {
    if let Some(name) = args.endpoint.as_deref() {
        if args.url.is_some() || (args.model.is_some() && own(name)) {
            let text = summonr_agent::add_endpoint(
                name,
                args.url.as_deref(),
                args.model.as_deref(),
                args.default,
            )
            .await?;
            println!("{text}");
            return Ok(());
        }
        if args.model.is_some()
            && !args.default
            && summonr_agent::auth_info(name).is_ok_and(|info| !info.own)
        {
            return Err(summonr_agent::Error::Config(format!(
                "`{name}` is built in; `summonr login {name} --default --model M` saves its model, or `summonr {name} --model M` uses one for a run"
            )));
        }
    }
    let Some(name) = args.endpoint.as_deref().filter(|_| args.default) else {
        return run_login(args).await;
    };
    if !summonr_agent::endpoint_set_up()
        .iter()
        .any(|ready| ready == name)
    {
        run_login(args).await?;
    }
    println!(
        "{}",
        summonr_agent::set_default_endpoint(name, args.model.as_deref())?
    );
    Ok(())
}

fn own(name: &str) -> bool {
    summonr_agent::auth_info(name).is_ok_and(|info| info.own)
}

fn logout(args: &Args) -> Result<i32> {
    let Some(name) = args.endpoint.as_deref() else {
        return Err(summonr_agent::Error::Config(
            "name an endpoint: `summonr logout NAME` or `summonr endpoint remove NAME`".to_string(),
        ));
    };
    let text = if args.remove {
        summonr_agent::remove_endpoint(name)?
    } else {
        summonr_agent::logout(name)?
    };
    println!("{text}");
    Ok(0)
}

fn profile(args: &Args) -> Result<()> {
    if args.all {
        print!(
            "{}",
            summonr_agent::profile(summonr_agent::ProfileScope::Recent(args.last))?
        );
    } else {
        print!(
            "{}",
            summonr_agent::profile(summonr_agent::ProfileScope::Session(&session_path(args)?))?
        );
    }
    Ok(())
}

async fn add(args: &Args) -> Result<i32> {
    if args.add_list {
        println!("{}", summonr_agent::Plugin::list()?);
        return Ok(0);
    }
    let Some(path) = args.prompt.first() else {
        return Err(summonr_agent::Error::Config(
            "summonr plugin install PATH|NAME@MARKETPLACE  (or plugin list)".to_string(),
        ));
    };
    let local = std::path::Path::new(path);
    let installed = if !local.exists() && path.contains('@') {
        summonr_agent::Marketplace::install(path)?
    } else {
        summonr_agent::Plugin::install(local)?
    };
    println!("{}", installed.report());
    for line in installed.check_servers(&workspace()?).await? {
        println!("{line}");
    }
    println!("restart summonr to load it");
    Ok(0)
}

fn uninstall(args: &Args) -> Result<i32> {
    let Some(name) = args.prompt.first() else {
        return Err(summonr_agent::Error::Config(
            "summonr plugin remove NAME".to_string(),
        ));
    };
    if !summonr_agent::Plugin::remove(name)? {
        return Err(summonr_agent::Error::Config(format!(
            "no plugin named {name} is installed"
        )));
    }
    println!("removed plugin {name}; restart summonr to unload it");
    Ok(0)
}

async fn update() -> Result<i32> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let line = match crate::update::run(&crate::update::Source::this_build(), &exe).await? {
        crate::update::Outcome::Current(version) => {
            format!("summonr {version} is the latest release")
        }
        crate::update::Outcome::Ahead { latest } => format!(
            "summonr {} is newer than the latest release, {latest}",
            env!("CARGO_PKG_VERSION")
        ),
        crate::update::Outcome::Updated { from, to } => {
            format!("Updated summonr {from} to {to} at {}", exe.display())
        }
    };
    println!("{line}");
    Ok(0)
}
