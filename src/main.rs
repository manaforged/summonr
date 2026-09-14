#[path = "main/cli.rs"]
mod cli;
#[path = "main/diagnostics.rs"]
mod diagnostics;
#[path = "main/modes.rs"]
mod modes;
#[path = "main/startup.rs"]
mod startup;

fn main() {
    #[cfg(windows)]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--sandbox-exec")
    {
        let args: Vec<_> = std::env::args_os().skip(2).collect();
        match summonr::sandbox_entry(&args) {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("summonr: {error}");
                std::process::exit(1);
            }
        }
    }
    run();
}

#[tokio::main]
async fn run() {
    match startup::run().await {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!(
                "{}: {error}",
                option_env!("CARGO_BIN_NAME").unwrap_or("summonr")
            );
            std::process::exit(1);
        }
    }
}
