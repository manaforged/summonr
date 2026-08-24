#[path = "main/cli.rs"]
mod cli;
#[path = "main/diagnostics.rs"]
mod diagnostics;
#[path = "main/modes.rs"]
mod modes;
#[path = "main/startup.rs"]
mod startup;

#[tokio::main]
async fn main() {
    match startup::run().await {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!(
                "{}: {error}",
                option_env!("CARGO_BIN_NAME").unwrap_or("summon")
            );
            std::process::exit(1);
        }
    }
}
