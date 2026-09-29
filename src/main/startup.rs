#[path = "startup/classify.rs"]
mod classify;
#[path = "startup/commands.rs"]
mod commands;
#[path = "startup/lifecycle.rs"]
mod lifecycle;
#[path = "startup/session.rs"]
mod session;
#[path = "startup/tool.rs"]
mod tool;

use summonr::Result;

use super::cli::Args;
use super::diagnostics;
use classify::run_classify;
use commands::{
    adopt, export_session_cmd, manage_worktrees, print_diff, print_sessions, print_workflows,
    probe_endpoints, run_login, session_path, workspace,
};
use lifecycle::launch;
use session::{SessionAction, build};

pub(super) async fn run(args: Args) -> Result<i32> {
    let log = if args.mail.is_some() {
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

async fn route(args: Args, log: Option<&diagnostics::Log>) -> Result<i32> {
    if let Some(code) = early(&args, log).await {
        return code;
    }
    let adopted = adopt(&args)?;
    if let Some(code) = account(&args).await {
        return code;
    }
    if let Some(code) = listing(&args) {
        return code;
    }
    launch(args, log, &adopted).await
}

async fn early(args: &Args, log: Option<&diagnostics::Log>) -> Option<Result<i32>> {
    if args.logs {
        return Some(diagnostics::print_logs(log.map(diagnostics::Log::path)));
    }
    if args.doctor {
        return Some(diagnostics::doctor(log.map(diagnostics::Log::path)));
    }
    if let Some(words) = &args.mail {
        return Some(
            std::env::current_dir()
                .map_err(Into::into)
                .and_then(|cwd| summonr::mail::cli::run(words, &cwd)),
        );
    }
    if let Some(tool) = &args.tool {
        return Some(tool::run(args, tool).await);
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
    if args.login {
        return Some(run_login(args).await.map(|()| 0));
    }
    if args.logout {
        return Some(logout(args));
    }
    if args.classify {
        return Some(run_classify(args).await);
    }
    if args.list {
        return Some(probe_endpoints().await.map(|()| 0));
    }
    None
}

fn logout(args: &Args) -> Result<i32> {
    let Some(name) = args.endpoint.as_deref() else {
        return Err(summonr::Error::Config(
            "logout needs an endpoint name".to_string(),
        ));
    };
    println!("{}", summonr::logout(name)?);
    Ok(0)
}

fn listing(args: &Args) -> Option<Result<i32>> {
    let done = |result: Result<()>| Some(result.map(|()| 0));
    if args.list_sessions {
        return done(print_sessions());
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
    if args.add {
        return Some(add(args));
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

fn add(args: &Args) -> Result<i32> {
    if args.add_list {
        println!("{}", summonr::list_extras()?);
        return Ok(0);
    }
    let Some(path) = args.prompt.first() else {
        return Err(summonr::Error::Config(
            "summonr add PATH  (or add --list)".to_string(),
        ));
    };
    println!("{}", summonr::add_bundle(std::path::Path::new(path))?);
    Ok(0)
}
