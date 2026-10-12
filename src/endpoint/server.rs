use toml_edit::{Item, Table, value};

use super::resolve::*;
use super::*;

pub async fn add_server(
    name: &str,
    url: Option<&str>,
    model: Option<&str>,
    default: bool,
) -> Result<String> {
    if !crate::skills::valid(name) {
        return Err(Error::Config(format!(
            "endpoint name `{name}`: letters, digits, `-`, and `_` only"
        )));
    }
    if builtin(name).is_some() {
        return Err(Error::Config(format!(
            "`{name}` is a built-in endpoint; pick another name for your server"
        )));
    }
    let url = match url {
        Some(url) => url.to_string(),
        None => saved_url(name)?,
    };
    let model = match model {
        Some(model) => model.to_string(),
        None => only_model(name, &url).await?,
    };
    let path = config::path()?;
    let replaced = config::remove_named(&path, config::Section::Endpoint, name)?;
    save(&path, name, &url, &model)?;
    if default {
        config::save_default_seat(&path, Some((name, &model)))?;
    }
    Ok(added(name, &url, &model, replaced))
}

fn saved_url(name: &str) -> Result<String> {
    config::file()?
        .endpoint
        .get(name)
        .and_then(|endpoint| endpoint.base_url.clone())
        .ok_or_else(|| {
            Error::Config(format!(
                "no endpoint named {name}; add it with `summonr login {name} --url URL [--model NAME]`"
            ))
        })
}

pub fn set_default(name: &str, model: Option<&str>) -> Result<String> {
    let endpoint = resolve(Some(name), model, Need::Seat)?;
    config::save_default_seat(&config::path()?, Some((name, &endpoint.model)))?;
    Ok(format!(
        "✓ {name} is the default endpoint: {}. Run `summonr` to start on it.",
        endpoint.model
    ))
}

async fn only_model(name: &str, url: &str) -> Result<String> {
    let mut file = (*config::file()?).clone();
    file.endpoint.insert(
        name.to_string(),
        FileEndpoint {
            base_url: Some(url.to_string()),
            ..FileEndpoint::default()
        },
    );
    let endpoint = seat(&file, name)
        .ok_or_else(|| Error::Config(format!("cannot read the endpoint `{name}`")))?;
    let endpoint = fill_key(endpoint, false)?;
    let models = crate::provider::Client::discover(&endpoint)
        .await
        .map_err(|error| {
            Error::Config(format!(
                "cannot list models at {url}: {error}; pass --model NAME to add it anyway"
            ))
        })?;
    match models.as_slice() {
        [only] => Ok(only.id.clone()),
        [] => Err(Error::Config(format!(
            "{url} lists no models; pass --model NAME"
        ))),
        many => Err(Error::Config(format!(
            "{url} serves {}; pass --model NAME",
            many.iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

pub fn remove_server(name: &str) -> Result<String> {
    if builtin(name).is_some() {
        return Err(Error::Config(format!(
            "`{name}` is built in and cannot be removed; `summonr logout {name}` forgets its key"
        )));
    }
    let removed = config::remove_named(&config::path()?, config::Section::Endpoint, name)?;
    super::auth::forget_key(name)?;
    if removed {
        Ok(format!(
            "✓ Removed the endpoint {name}.{}",
            clear_default(name)?
        ))
    } else {
        Err(Error::Config(format!(
            "no endpoint named {name} in {}",
            config::tilde(&config::path()?)
        )))
    }
}

fn save(path: &std::path::Path, name: &str, url: &str, model: &str) -> Result<bool> {
    let mut table = Table::new();
    table["base_url"] = value(url);
    table["model"] = value(model);
    config::save_named(path, config::Section::Endpoint, name, Item::Table(table))
}

fn added(name: &str, url: &str, model: &str, replaced: bool) -> String {
    let verb = if replaced { "Updated" } else { "Added" };
    let default = config::file().ok().and_then(|file| file.default.clone());
    if default.as_deref() == Some(name) {
        return format!("✓ {verb} {name}: {model} at {url}. It is the default; run `summonr`.");
    }
    format!(
        "✓ {verb} {name}: {model} at {url}. Run `summonr {name}`, or `summonr login {name} --default` to start on it by default."
    )
}

fn clear_default(name: &str) -> Result<String> {
    if config::file()?.default.as_deref() != Some(name) {
        return Ok(String::new());
    }
    config::save_default_seat(&config::path()?, None)?;
    Ok(" It was the default, so `default` and `default_model` are cleared; the next run picks the first endpoint that is set up.".to_string())
}
