#[path = "main/cli.rs"]
mod cli;
#[path = "main/diagnostics.rs"]
mod diagnostics;
#[path = "main/modes.rs"]
mod modes;
#[path = "main/startup.rs"]
mod startup;

fn main() {
    #[cfg(all(windows, feature = "windows-sandbox"))]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--sandbox-exec")
    {
        let args: Vec<_> = std::env::args_os().skip(2).collect();
        std::process::exit(summonr::sandbox_entry(&args).unwrap_or_else(|error| fail(&error)));
    }
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "askpass")
    {
        let args: Vec<_> = std::env::args_os().skip(2).collect();
        std::process::exit(summonr::askpass::client(&args));
    }
    let profile = std::env::args().nth(1).as_deref() == Some("--profile-tui");
    let args = (!profile)
        .then(cli::parse)
        .transpose()
        .unwrap_or_else(|error| fail(&error));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap_or_else(|error| fail(&error));
    let session = async {
        tokio::select! {
            result = run(args) => result,
            code = interrupted() => Ok(code),
        }
    };
    match runtime.block_on(session) {
        Ok(code) => {
            summonr::stop_children();
            std::process::exit(code);
        }
        Err(error) => fail(&error),
    }
}

#[cfg(unix)]
async fn interrupted() -> i32 {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut interrupt), Ok(mut terminate), Ok(mut hangup)) = (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
        signal(SignalKind::hangup()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = interrupt.recv() => 130,
        _ = terminate.recv() => 143,
        _ = hangup.recv() => 129,
    }
}

#[cfg(not(unix))]
async fn interrupted() -> i32 {
    if tokio::signal::ctrl_c().await.is_ok() {
        return 130;
    }
    std::future::pending().await
}

async fn run(args: Option<cli::Args>) -> summonr::Result<i32> {
    match args {
        Some(args) => {
            summonr::check_config()?;
            startup::run(args).await
        }
        None => summonr::tui::profile(std::env::args().skip(2)).map(|()| 0),
    }
}

fn fail(error: &dyn std::fmt::Display) -> ! {
    eprintln!(
        "{}: {error}",
        option_env!("CARGO_BIN_NAME").unwrap_or("summonr")
    );
    summonr::stop_children();
    std::process::exit(1)
}
