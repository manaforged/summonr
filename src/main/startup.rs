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

use super::cli::Args;
use super::diagnostics;
use classify::run_classify;
use commands::{
    adopt, export_session_cmd, grep_sessions, manage_worktrees, mcp, print_diff, print_sessions,
    print_workflows, session_path, workspace,
};
use lifecycle::launch;
use login::run_login;
use session::{SessionAction, build};

fn store_command(args: &Args) -> bool {
    args.mail.is_some() || args.cron.is_some() || args.gateway.is_some()
}

pub(super) async fn run(args: Args) -> Result<i32> {
    let log = if store_command(&args) {
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
    if args.logs {
        return Some(diagnostics::print_logs(log.map(diagnostics::Log::path)));
    }
    if args.report {
        return Some(diagnostics::report(log.map(diagnostics::Log::path)));
    }
    if args.doctor {
        return Some(diagnostics::doctor(log.map(diagnostics::Log::path)));
    }
    let in_cwd = |run: &dyn Fn(&std::path::Path) -> Result<i32>| {
        Some(
            std::env::current_dir()
                .map_err(Into::into)
                .and_then(|cwd| run(&cwd)),
        )
    };
    if let Some(words) = &args.mail {
        return in_cwd(&|cwd| summonr::mail::cli::run(words, cwd));
    }
    if let Some(words) = &args.cron {
        return in_cwd(&|cwd| summonr::cron::cli::run(words, cwd));
    }
    if let Some(words) = &args.gateway {
        return in_cwd(&|cwd| summonr::gateway::cli::run(words, cwd, &login::read_secret));
    }
    if let Some(tool) = &args.tool {
        return Some(tool::run(tool).await);
    }
    if !args.acp {
        return None;
    }
    Some(
        summonr::acp::serve(
            args.endpoint.as_deref(),
            args.model.as_deref(),
            |endpoint, client, cwd| {
                build(args, endpoint, client, cwd, None, SessionAction::Create)
                    .map(summonr::Loop::with_interactive)
            },
        )
        .await,
    )
}

async fn account(args: &Args) -> Option<Result<i32>> {
    if args.login
        && let Some(name) = args.endpoint.as_deref()
        && (args.url.is_some() || (args.model.is_some() && own(name)))
    {
        return Some(
            summonr::add_endpoint(
                name,
                args.url.as_deref(),
                args.model.as_deref(),
                args.default,
            )
            .await
            .map(|text| println!("{text}"))
            .map(|()| 0),
        );
    }
    if args.login
        && let Some(name) = args.endpoint.as_deref()
        && args.model.is_some()
        && !args.default
        && summonr::auth_info(name).is_ok_and(|info| !info.own)
    {
        return Some(Err(summonr::Error::Config(format!(
            "`{name}` is built in; `summonr login {name} --default --model M` saves its model, or `summonr {name} --model M` uses one for a run"
        ))));
    }
    if args.login {
        return Some(login(args).await.map(|()| 0));
    }
    if args.logout {
        return Some(logout(args));
    }
    if args.classify {
        return Some(run_classify(args).await);
    }
    if args.list {
        return Some(endpoints::list(args.all, args.live).await.map(|()| 0));
    }
    if let Some(command) = &args.mcp {
        return Some(mcp(command, args.json).await.map(|()| 0));
    }
    None
}

async fn login(args: &Args) -> Result<()> {
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
    if args.list_sessions {
        return done(match args.grep.as_deref() {
            Some(pattern) => grep_sessions(pattern),
            None => print_sessions(),
        });
    }
    if args.export {
        return done(export_session_cmd(args));
    }
    if args.profile {
        return done(profile(args));
    }
    if args.list_workflows {
        return done(print_workflows());
    }
    if args.list_modules {
        return done(
            workspace()
                .and_then(|root| summonr::modules::Switches::load(&root))
                .map(|switches| println!("{}", switches.table())),
        );
    }
    if args.uninstall {
        return Some(uninstall(args));
    }
    if args.add {
        return Some(add(args).await);
    }
    if args.list_worktrees {
        return done(manage_worktrees(args));
    }
    if args.diff {
        return done(print_diff(args));
    }
    None
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
