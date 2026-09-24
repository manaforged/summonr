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
    match runtime.block_on(run(args)) {
        Ok(code) => std::process::exit(code),
        Err(error) => fail(&error),
    }
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
    std::process::exit(1)
}
