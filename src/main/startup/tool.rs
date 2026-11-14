use summonr_agent::Result;

use super::commands::workspace;

pub(super) async fn run(name: &str, arguments: &str) -> Result<i32> {
    let outcome = summonr_agent::run_tool(&workspace()?, name, arguments).await?;
    println!("{}", outcome.text);
    Ok(if outcome.success { 0 } else { 1 })
}
