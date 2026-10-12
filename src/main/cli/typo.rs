use summonr::{Error, Result, is_endpoint};

use super::flags::COMMANDS;

pub(super) fn refuse(words: &[String]) -> Result<()> {
    let [word] = words else {
        return Ok(());
    };
    if word.starts_with('-') || is_endpoint(word) || COMMANDS.iter().any(|(name, _)| name == word) {
        return Ok(());
    }
    let names: Vec<String> = COMMANDS
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    summonr::closest_endpoint(word, &names, summonr::EndpointNear::Typo).map_or(Ok(()), |near| {
        Err(Error::Config(format!(
            "unknown command `{word}`; did you mean `{near}`? To send it as a prompt, use -p."
        )))
    })
}
