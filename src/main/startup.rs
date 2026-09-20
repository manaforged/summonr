#[path = "startup/commands.rs"]
mod commands;
#[path = "startup/inference.rs"]
mod inference;
#[path = "startup/lifecycle.rs"]
mod lifecycle;
#[path = "startup/session.rs"]
mod session;

use summonr::Result;

use super::cli::{self, Args};
use super::diagnostics;
use commands::{
    adopt, export_session_cmd, manage_worktrees, print_diff, print_sessions, print_workflows,
    probe_endpoints, run_login, session_path, workspace,
};
use inference::run_decide;
use lifecycle::launch;
use session::{SessionAction, build};

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
    if args.add {
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
