use summonr_agent::{Error, Result, is_endpoint};

use super::commands::COMMANDS;
use super::flags::ALIASES;

pub(super) fn refuse(words: &[String]) -> Result<()> {
    let [word] = words else {
        return Ok(());
    };
    if word.starts_with('-')
        || is_endpoint(word)
        || COMMANDS.iter().any(|row| row.name == word)
        || ALIASES.iter().any(|(old, _)| old == word)
    {
        return Ok(());
    }
    let names: Vec<String> = COMMANDS.iter().map(|row| row.name.to_string()).collect();
    summonr_agent::closest_endpoint(word, &names, summonr_agent::EndpointNear::Typo).map_or(
        Ok(()),
        |near| {
            Err(Error::Setup(format!(
                "unknown command `{word}`; did you mean `{near}`? To send it as a prompt, use -p."
            )))
        },
    )
}
