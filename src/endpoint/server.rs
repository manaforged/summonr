use toml_edit::{Item, Table, value};

use super::resolve::*;
use super::*;

pub async fn add_server(name: &str, url: &str, model: Option<&str>) -> Result<String> {
    if !crate::skills::valid(name) {
        return Err(Error::Config(format!(
            "endpoint name `{name}`: letters, digits, `-`, and `_` only"
        )));
    }
    if is_named(name) {
        return Err(Error::Config(format!(
            "`{name}` is already an endpoint; pick another name or run `summonr logout {name} --remove` first"
        )));
    }
    let path = config::path()?;
    save(&path, name, url, model)?;
    if model.is_some() {
        return Ok(added(name, url, model.unwrap_or_default()));
    }
    let listed = match resolve(Some(name), None, Need::Config) {
        Ok(endpoint) => crate::provider::Client::discover(&endpoint).await,
        Err(error) => Err(error),
    };
    config::remove_named(&path, config::Section::Endpoint, name)?;
    let models = listed.map_err(|error| {
        Error::Config(format!(
            "cannot list models at {url}: {error}; pass --model NAME to add it anyway"
        ))
    })?;
    match models.as_slice() {
        [only] => {
            save(&path, name, url, Some(&only.id))?;
            Ok(added(name, url, &only.id))
        }
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
        Ok(format!("✓ Removed the endpoint {name}."))
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

fn added(name: &str, url: &str, model: &str) -> String {
    format!(
        "✓ Added {name}: {model} at {url}. Run `summonr {name}`, or set `default = \"{name}\"` in {}.",
        config::path().map_or_else(|_| "config.toml".to_string(), |path| config::tilde(&path))
    )
}
