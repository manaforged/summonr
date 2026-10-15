use summonr::Result;

use super::commands::workspace;

pub(super) async fn run(tool: &[String]) -> Result<i32> {
    let Some(name) = tool.first() else {
        return Err(summonr::Error::Config(
            "summonr tool NAME [JSON]: invoke a tool directly".to_string(),
        ));
    };
    let arguments = match tool.get(1..) {
        Some(rest) if !rest.is_empty() => rest.join(" "),
        Some(_) | None => "{}".to_string(),
    };
    let outcome = summonr::run_tool(&workspace()?, name, &arguments).await?;
    println!("{}", outcome.text);
    Ok(if outcome.success { 0 } else { 1 })
}
