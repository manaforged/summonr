use super::resolve::*;
use super::*;

pub fn auth_info(name: &str) -> Result<AuthInfo> {
    let endpoint = resolve(Some(name), None, Need::Config)?;
    let offered = offered_login(name, &endpoint);
    let mut info = AuthInfo {
        name: name.to_string(),
        flow: endpoint.oauth.as_ref().map(flow).unwrap_or_default(),
        login: offered.as_ref().map(flow).unwrap_or_default(),
        key: takes_key(&endpoint) || (preset(name).is_none() && offered.is_none()),
        plan: offered
            .as_ref()
            .and_then(|row| row.label.clone())
            .unwrap_or_default(),
        configured: endpoint.api_key.is_some(),
        source: endpoint.source,
        setup: preset(name)
            .map(|preset| preset.missing.replace("NAME", name))
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| format!("run `summonr login {name}` to store a key")),
        env: (endpoint.source == Some(Source::Env))
            .then(|| preset(name).or_else(|| endpoint.base_url.as_deref().and_then(hosted)))
            .flatten()
            .and_then(|row| {
                row.env_keys
                    .iter()
                    .copied()
                    .find(|key| env_key(&[key]).is_some())
            }),
        group: login_group(name, &endpoint),
    };
    if let Some(spec) = endpoint.oauth.as_ref().filter(|spec| flow(spec).signs_in()) {
        info.setup = match (flow(spec), info.key) {
            (_, true) => format!(
                "run `summonr login {name}` to sign in with {} or paste a key",
                info.plan
            ),
            (LoginFlow::Browser, false) => {
                format!("run `summonr login {name}` to sign in through the browser")
            }
            (LoginFlow::Device | LoginFlow::Key, false) => {
                format!("run `summonr login {name}` for device authentication")
            }
        };
        info.configured = match logged_in(spec, name) {
            Ok(ready) => ready,
            Err(error) => {
                info.setup = format!("{error}; {}", info.setup);
                false
            }
        };
    }
    Ok(info)
}

fn offered_login(name: &str, endpoint: &Endpoint) -> Option<crate::config::OAuth> {
    endpoint
        .oauth
        .clone()
        .or_else(|| builtin(name).and_then(|row| row.oauth))
        .or_else(|| {
            config::file()
                .ok()
                .and_then(|file| file.endpoint.get(name).and_then(|row| row.auth.clone()))
        })
}

pub fn login_seat(name: &str) -> Result<Endpoint> {
    let mut endpoint = resolve(Some(name), None, Need::Config)?;
    if endpoint.oauth.is_none() {
        endpoint.oauth = offered_login(name, &endpoint);
        endpoint.api_key = None;
        endpoint.source = None;
    }
    Ok(endpoint)
}

pub(crate) fn save_key(name: &str, key: &str) -> Result<Source> {
    let info = auth_info(name)?;
    if info.login.signs_in() && !info.key {
        return Err(Error::Config(format!(
            "{name} uses a login token, not an API key — run `summonr login {name}` without --key"
        )));
    }
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::Config("API key cannot be empty".to_string()));
    }
    let source = secret::set(name, key)?;
    config::remove_key(&config::path()?, name)?;
    Ok(source)
}

pub async fn check_key(name: &str, key: &str) -> Result<Source> {
    let mut endpoint = resolve(Some(name), None, Need::Config)?;
    endpoint.api_key = Some(key.trim().to_string());
    endpoint.source = None;
    if let Err(error) = crate::provider::Client::discover(&endpoint).await
        && crate::provider::Client::state(&error) == State::KeyRejected
    {
        return Err(Error::provider(
            crate::error::ProviderFailureKind::Authentication,
            format!("{name} rejected the key"),
        ));
    }
    save_key(name, key)
}

pub(crate) fn forget_key(name: &str) -> Result<bool> {
    let stored = secret::forget(name)?;
    let token = secret::forget(&format!("oauth:{name}"))?;
    let config = config::remove_key(&config::path()?, name)?;
    Ok(stored || token || config)
}

pub fn logout(name: &str) -> Result<String> {
    let head = if forget_key(name)? {
        format!("✓ Signed out of {name}.")
    } else {
        format!("Nothing is stored for {name}.")
    };
    Ok(match auth_info(name)?.source {
        Some(source) => format!("{head} It now uses the {} key.", source.label()),
        None => head,
    })
}

pub fn adopt_keys() -> Result<Vec<String>> {
    let path = config::path()?;
    let store = secret::Store::discover()?;
    let file = config::file()?;
    let mut moved = Vec::new();
    for (name, row) in &file.endpoint {
        let Some(key) = row
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        else {
            continue;
        };
        if store.get(name)?.is_none() {
            store.set(name, key)?;
        }
        config::remove_key(&path, name)?;
        moved.push(name.clone());
    }
    Ok(moved)
}

pub(crate) fn ready(need: Need) -> (Vec<Endpoint>, Vec<String>) {
    let mut names = match names() {
        Ok(names) => names,
        Err(error) => return (Vec::new(), vec![error.to_string()]),
    };
    if let Ok(endpoint) = resolve(None, None, need)
        && !names.contains(&endpoint.name)
    {
        names.insert(0, endpoint.name);
    }
    let mut endpoints = Vec::new();
    let mut errors = Vec::new();
    for name in names {
        let endpoint = match resolve(Some(&name), None, need) {
            Ok(endpoint) if endpoint.wire == Wire::Decisions => continue,
            Ok(endpoint) => endpoint,
            Err(_) => {
                if let Err(error) = resolve(Some(&name), None, need.without_key())
                    && !errors.contains(&error.to_string())
                {
                    errors.push(error.to_string());
                }
                continue;
            }
        };
        let available = endpoint
            .oauth
            .as_ref()
            .map_or(Ok(true), |spec| logged_in(spec, &endpoint.name));
        match available {
            Ok(true) => endpoints.push(endpoint),
            Ok(false) => {}
            Err(error) => errors.push(format!("{}: {error}", endpoint.name)),
        }
    }
    (endpoints, errors)
}

pub fn first_classify() -> Option<String> {
    names().unwrap_or_default().into_iter().find(|name| {
        resolve(Some(name), None, Need::Key).is_ok_and(|endpoint| endpoint.wire == Wire::Decisions)
    })
}

pub(crate) fn step(current: &str, forward: bool) -> Option<String> {
    let (endpoints, _) = ready(Need::Seat);
    if endpoints.is_empty() {
        return None;
    }
    let index = endpoints
        .iter()
        .position(|endpoint| endpoint.name == current);
    let target = match (index, forward) {
        (Some(index), true) => (index + 1) % endpoints.len(),
        (Some(index), false) => (index + endpoints.len() - 1) % endpoints.len(),
        (None, true) => 0,
        (None, false) => endpoints.len() - 1,
    };
    (endpoints[target].name != current).then(|| endpoints[target].name.clone())
}

#[cfg(test)]
pub(crate) fn catalog(wire: Wire) -> Vec<String> {
    let mut models = Vec::new();
    for row in presets() {
        if row.wire != wire {
            continue;
        }
        if !models.iter().any(|model| model == row.model) {
            models.push(row.model.to_string());
        }
        for (_, canon) in row.aliases {
            if !models.iter().any(|model| model == *canon) {
                models.push((*canon).to_string());
            }
        }
    }
    models
}
