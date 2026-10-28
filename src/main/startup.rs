#[path = "startup/classify.rs"]
mod classify;
#[path = "startup/commands.rs"]
mod commands;
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

use summonr::Result;

use super::cli::{Args, Verb};
use super::diagnostics;
use classify::run_classify;
use commands::{
    adopt, export_session_cmd, grep_sessions, manage_worktrees, mcp, print_diff, print_sessions,
    print_workflows, session_path, workspace,
};
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
            (Err(error), Some((reason, summonr::AbortCode::Error)))
        }
        Err(code) => (
            Ok(code),
            Some((signal(code), summonr::AbortCode::Interrupted)),
        ),
    };
    if events && let Some((reason, code)) = &abort {
        super::modes::print_json(&summonr::Event::Aborted {
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
    summonr::check_config()?;
    if let Some(code) = early(&args, log).await {
        return code;
    }
    let adopted = adopt(&args)?;
    if let Some(code) = account(&args).await {
        return code;
    }
    if let Some(code) = listing(&args).await {
        return code;
    }
    launch(&args, log, &adopted).await
}

async fn early(args: &Args, log: Option<&diagnostics::Log>) -> Option<Result<i32>> {
    let in_cwd = |run: &dyn Fn(&std::path::Path) -> Result<i32>| {
        Some(
            std::env::current_dir()
                .map_err(Into::into)
                .and_then(|cwd| run(&cwd)),
        )
    };
    let path = log.map(diagnostics::Log::path);
    match args.verb.as_ref()? {
        Verb::Doctor if args.logs => Some(diagnostics::print_logs(path)),
        Verb::Doctor if args.report => Some(diagnostics::report(path)),
        Verb::Doctor => Some(diagnostics::doctor(path)),
        Verb::Mail(words) => in_cwd(&|cwd| summonr::mail::cli::run(words, cwd)),
        Verb::Cron(words) => in_cwd(&|cwd| summonr::cron::cli::run(words, cwd)),
        Verb::Gateway(words) => {
            in_cwd(&|cwd| summonr::gateway::cli::run(words, cwd, &login::read_secret))
        }
        Verb::Tool(tool) => Some(tool::run(tool).await),
        Verb::Acp => Some(acp(args).await),
        _ => None,
    }
}

async fn acp(args: &Args) -> Result<i32> {
    summonr::acp::serve(
        args.endpoint.as_deref(),
        args.model.as_deref(),
        |endpoint, client, cwd| {
            build(args, endpoint, client, cwd, None, SessionAction::Create)
                .map(summonr::Loop::with_interactive)
        },
    )
    .await
}

async fn account(args: &Args) -> Option<Result<i32>> {
    let done = |result: Result<()>| Some(result.map(|()| 0));
    match args.verb.as_ref()? {
        Verb::Login => done(login(args).await),
        Verb::Logout => Some(logout(args)),
        Verb::Classify => Some(run_classify(args).await),
        Verb::Endpoints => done(endpoints::list(args.all, args.live).await),
        Verb::Mcp(command) => done(mcp(command, args.json).await),
        _ => None,
    }
}

async fn login(args: &Args) -> Result<()> {
    if let Some(name) = args.endpoint.as_deref() {
        if args.url.is_some() || (args.model.is_some() && own(name)) {
            let text = summonr::add_endpoint(
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
            && summonr::auth_info(name).is_ok_and(|info| !info.own)
        {
            return Err(summonr::Error::Config(format!(
                "`{name}` is built in; `summonr login {name} --default --model M` saves its model, or `summonr {name} --model M` uses one for a run"
            )));
        }
    }
    let Some(name) = args.endpoint.as_deref().filter(|_| args.default) else {
        return run_login(args).await;
    };
    if !summonr::endpoint_set_up().iter().any(|ready| ready == name) {
        run_login(args).await?;
    }
    println!(
        "{}",
        summonr::set_default_endpoint(name, args.model.as_deref())?
    );
    Ok(())
}

fn own(name: &str) -> bool {
    summonr::auth_info(name).is_ok_and(|info| info.own)
}

fn logout(args: &Args) -> Result<i32> {
    let Some(name) = args.endpoint.as_deref() else {
        return Err(summonr::Error::Config(
            "logout needs an endpoint name".to_string(),
        ));
    };
    let text = if args.remove {
        summonr::remove_endpoint(name)?
    } else {
        summonr::logout(name)?
    };
    println!("{text}");
    Ok(0)
}

async fn listing(args: &Args) -> Option<Result<i32>> {
    let done = |result: Result<()>| Some(result.map(|()| 0));
    match args.verb.as_ref()? {
        Verb::Sessions => done(match args.grep.as_deref() {
            Some(pattern) => grep_sessions(pattern),
            None => print_sessions(),
        }),
        Verb::Export => done(export_session_cmd(args)),
        Verb::Profile => done(profile(args)),
        Verb::Workflows => done(print_workflows()),
        Verb::Modules => done(
            workspace()
                .and_then(|root| summonr::modules::Switches::load(&root))
                .map(|switches| println!("{}", switches.table())),
        ),
        Verb::Uninstall => Some(uninstall(args)),
        Verb::Update => Some(update().await),
        Verb::Install => Some(add(args).await),
        Verb::Worktrees => done(manage_worktrees(args)),
        Verb::Diff => done(print_diff(args)),
        _ => None,
    }
}

fn profile(args: &Args) -> Result<()> {
    if args.all {
        print!(
            "{}",
            summonr::profile(summonr::ProfileScope::Recent(args.last))?
        );
    } else {
        print!(
            "{}",
            summonr::profile(summonr::ProfileScope::Session(&session_path(args)?))?
        );
    }
    Ok(())
}

async fn add(args: &Args) -> Result<i32> {
    if args.add_list {
        println!("{}", summonr::list_plugins()?);
        return Ok(0);
    }
    let Some(path) = args.prompt.first() else {
        return Err(summonr::Error::Config(
            "summonr install PATH  (or install --list)".to_string(),
        ));
    };
    let installed = summonr::install_plugin(std::path::Path::new(path))?;
    println!("{}", installed.report);
    for line in summonr::check_plugin_servers(&installed, &workspace()?).await? {
        println!("{line}");
    }
    println!("restart summonr to load it");
    Ok(0)
}

fn uninstall(args: &Args) -> Result<i32> {
    let Some(name) = args.prompt.first() else {
        return Err(summonr::Error::Config("summonr uninstall NAME".to_string()));
    };
    if !summonr::uninstall_plugin(name)? {
        return Err(summonr::Error::Config(format!(
            "no plugin named {name} is installed"
        )));
    }
    println!("removed plugin {name}; restart summonr to unload it");
    Ok(0)
}

async fn update() -> Result<i32> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let line = match summonr::update::run(&summonr::update::Source::this_build(), &exe).await? {
        summonr::update::Outcome::Current(version) => {
            format!("summonr {version} is the latest release")
        }
        summonr::update::Outcome::Ahead { latest } => format!(
            "summonr {} is newer than the latest release, {latest}",
            env!("CARGO_PKG_VERSION")
        ),
        summonr::update::Outcome::Updated { from, to } => {
            format!("Updated summonr {from} to {to} at {}", exe.display())
        }
    };
    println!("{line}");
    Ok(0)
}
