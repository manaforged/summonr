use summonr::Result;

use super::commands::workspace;
use crate::cli::Args;

pub(super) async fn run(args: &Args, tool: &[String]) -> Result<i32> {
    let Some(name) = tool.first() else {
        return Err(summonr::Error::Config(
            "summonr tool NAME [JSON] — invoke a tool directly".to_string(),
        ));
    };
    let arguments = match tool.get(1..) {
        Some(rest) if !rest.is_empty() => rest.join(" "),
        Some(_) | None => "{}".to_string(),
    };
    let outcome = summonr::run_tool(
        &workspace()?,
        name,
        &arguments,
        args.sandbox == Some(true),
        !args.network,
    )
    .await?;
    println!("{}", outcome.text);
    Ok(if outcome.success { 0 } else { 1 })
}
