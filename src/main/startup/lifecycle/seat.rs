use summonr::{Client, Endpoint, Error, Need, Result};

use super::{Ui, use_tui};
use crate::cli::Args;

pub(super) async fn seat(args: &Args, adopted: &[String]) -> Result<Endpoint> {
    let need = if use_tui(args) {
        Need::Model
    } else {
        Need::Seat
    };
    let (name, model) = (args.endpoint.as_deref(), args.model.as_deref());
    let endpoint = match summonr::resolve_endpoint(name, model, need) {
        Ok(endpoint) => endpoint,
        Err(error) => return Err(served(name, model, error).await),
    };
    if endpoint.source == Some(summonr::secret::Source::Config)
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
    let Error::Config(text) = error else {
        return error;
    };
    let Ok(bare) = summonr::resolve_endpoint(name, model, Need::Config) else {
        return Error::Config(text);
    };
    if !bare.model.is_empty() {
        return Error::Config(text);
    }
    match Client::discover(&bare).await {
        Ok(models) => {
            let ids: Vec<String> = models.into_iter().map(|row| row.id).collect();
            Error::Config(format!("{text}; {} serves {}", bare.name, ids.join(", ")))
        }
        Err(unreachable) => unreachable,
    }
}

pub(super) fn ask_login(ui: &Ui, endpoint: &Endpoint, named: bool) {
    if let Some(ui) = &ui.0
        && endpoint.api_key.is_none()
        && endpoint.oauth.is_none()
    {
        let (ui, seat) = (ui.clone(), endpoint.clone());
        drop(tokio::spawn(async move {
            let note = if !seat.keyless() && named {
                format!(
                    "{} has no key yet. Paste its key, or pick another provider.",
                    seat.name
                )
            } else if !seat.keyless() && summonr::endpoint_set_up().is_empty() {
                "No provider is set up yet. Pick one and paste its key.".to_string()
            } else if !seat.keyless() {
                "Pick a model to start.".to_string()
            } else {
                match Client::discover(&seat).await {
                    Err(error) if Client::state(&error) == summonr::EndpointState::KeyRejected => {
                        format!(
                            "{} rejected the request. Paste its key, or pick another provider.",
                            seat.name
                        )
                    }
                    Ok(_) | Err(_) => return,
                }
            };
            ui.open_login(note);
        }));
    }
}
