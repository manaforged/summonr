use toml_edit::{Item, Table, value};

use super::resolve::*;
use super::*;

pub async fn add_server(name: &str, url: &str, model: Option<&str>) -> Result<String> {
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
    let model = match model {
        Some(model) => model.to_string(),
        None => only_model(name, url).await?,
    };
    let path = config::path()?;
    let replaced = config::remove_named(&path, config::Section::Endpoint, name)?;
    save(&path, name, url, Some(&model))?;
    Ok(added(name, url, &model, replaced))
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

fn save(path: &std::path::Path, name: &str, url: &str, model: Option<&str>) -> Result<bool> {
    let mut table = Table::new();
    table["base_url"] = value(url);
    if let Some(model) = model {
        table["model"] = value(model);
    }
    config::save_named(path, config::Section::Endpoint, name, Item::Table(table))
}

fn added(name: &str, url: &str, model: &str, replaced: bool) -> String {
    let verb = if replaced { "Updated" } else { "Added" };
    let config =
        config::path().map_or_else(|_| "config.toml".to_string(), |path| config::tilde(&path));
    let default = config::file().ok().and_then(|file| file.default.clone());
    if default.as_deref() == Some(name) {
        return format!("✓ {verb} {name}: {model} at {url}. Run `summonr {name}`.");
    }
    format!(
        "✓ {verb} {name}: {model} at {url}. Run `summonr {name}`, or set `default = \"{name}\"` in {config}."
    )
}

fn clear_default(name: &str) -> Result<String> {
    if config::file()?.default.as_deref() != Some(name) {
        return Ok(String::new());
    }
    config::save_default_seat(&config::path()?, None)?;
    Ok(" It was the default, so `default` and `default_model` are cleared; the next run picks the first endpoint that is set up.".to_string())
}
