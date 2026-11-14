use summonr_agent::{Client, Endpoint, Error, Requirement, Result};
use summonr_core::error::ErrorKind;

use super::{Ui, use_tui};
use crate::entry::cli::Args;

pub(super) async fn selection(args: &Args, adopted: &[String]) -> Result<Endpoint> {
    let need = if use_tui(args) {
        Requirement::Model
    } else {
        Requirement::ModelAndKey
    };
    let (name, model) = (args.endpoint.as_deref(), args.model.as_deref());
    let endpoint = match summonr_agent::resolve_endpoint(name, model, need) {
        Ok(endpoint) => endpoint,
        Err(error) => return Err(served(name, model, error).await),
    };
    if endpoint.source == Some(summonr_agent::secret::Source::Config)
        && !args.json
        && !adopted.contains(&endpoint.name)
    {
        eprintln!(
            "the key for {} is plaintext in config.toml; `summonr login {} --key` moves it into the credential store",
            endpoint.name, endpoint.name
        );
    }
    Ok(endpoint)
}

async fn served(name: Option<&str>, model: Option<&str>, error: Error) -> Error {
    if error.kind() != ErrorKind::Config {
        return error;
    }
    let Ok(bare) = summonr_agent::resolve_endpoint(name, model, Requirement::Config) else {
        return error;
    };
    if !bare.model.is_empty() {
        return error;
    }
    match Client::discover(&bare).await {
        Ok(models) => {
            let ids: Vec<String> = models.into_iter().map(|row| row.id).collect();
            Error::Config(format!(
                "{}; {} serves {}",
                error.detail(),
                bare.name,
                ids.join(", ")
            ))
        }
        Err(unreachable) => unreachable,
    }
}

pub(super) fn ask_login(ui: &Ui, endpoint: &Endpoint, named: bool) {
    if let Some(ui) = &ui.0
        && endpoint.api_key.is_none()
        && endpoint.oauth.is_none()
    {
        let (ui, selection) = (ui.clone(), endpoint.clone());
        drop(tokio::spawn(async move {
            let note = if !selection.keyless() && named {
                format!(
                    "{} has no key yet. Paste its key, or pick another provider.",
                    selection.name
                )
            } else if !selection.keyless() && summonr_agent::endpoint_set_up().is_empty() {
                "No provider is set up yet. Pick one to start.".to_string()
            } else if !selection.keyless() {
                "Pick a model to start.".to_string()
            } else {
                match Client::discover(&selection).await {
                    Err(error)
                        if Client::state(&error) == summonr_agent::EndpointState::KeyRejected =>
                    {
                        format!(
                            "{} rejected the request. Paste its key, or pick another provider.",
                            selection.name
                        )
                    }
                    Ok(_) | Err(_) => return,
                }
            };
            ui.open_login(note);
        }));
    }
}
