use summonr_agent::mcp::McpLaunch;

/// Builds a server launch from the form: a command or URL, then comma-separated `KEY=VALUE` pairs.
pub(super) fn launch(target: &str, extra: &str) -> Result<McpLaunch, String> {
    let pairs = extra
        .split(',')
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .map(|word| summonr_agent::mcp::pair(word).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if target.starts_with("http://") || target.starts_with("https://") {
        return Ok(McpLaunch::Http {
            url: target.to_owned(),
            headers: pairs,
            auth: None,
        });
    }
    let words = summonr_agent::policy::shell_words(target);
    let (command, args) = words
        .split_first()
        .ok_or_else(|| "Enter a command or URL".to_owned())?;
    Ok(McpLaunch::Stdio {
        command: command.clone(),
        args: args.to_vec(),
        env: pairs,
    })
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
