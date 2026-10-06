use super::*;

pub(super) fn fill_key(mut endpoint: Endpoint, require_key: bool) -> Result<Endpoint> {
    let offered = match endpoint.oauth.take() {
        Some(login) if takes_key(&endpoint) && !logged_in(&login, &endpoint.name)? => Some(login),
        Some(login) => {
            endpoint.oauth = Some(login);
            return Ok(endpoint);
        }
        None => None,
    };
    if let Some((key, source)) = known_key(&endpoint)? {
        endpoint.api_key = Some(key);
        endpoint.source = Some(source);
        return Ok(endpoint);
    }
    fallback_key(endpoint, require_key, offered)
}

fn known_key(endpoint: &Endpoint) -> Result<Option<(String, Source)>> {
    let custom = preset(&endpoint.name).is_none();
    if custom && let Some(key) = &endpoint.api_key {
        return Ok(Some((key.clone(), Source::Config)));
    }
    let own = if custom {
        secret::get(&endpoint.name)?
    } else {
        None
    };
    let row = preset(&endpoint.name)
        .filter(|row| own_base(endpoint, row))
        .or_else(|| endpoint.base_url.as_deref().and_then(hosted));
    if own.is_none()
        && let Some(key) = row.and_then(|row| env_key(row.env_keys))
    {
        return Ok(Some((key, Source::Env)));
    }
    let stored = if custom {
        own
    } else {
        secret::get(&endpoint.name)?
    };
    Ok(stored.or_else(|| peer_key(endpoint, row)))
}

pub(super) fn takes_key(endpoint: &Endpoint) -> bool {
    preset(&endpoint.name).is_some_and(|row| !row.env_keys.is_empty())
}

fn fallback_key(
    mut endpoint: Endpoint,
    require_key: bool,
    offered: Option<crate::config::OAuth>,
) -> Result<Endpoint> {
    if endpoint.api_key.is_some() {
        endpoint.source = Some(Source::Config);
        return Ok(endpoint);
    }
    if offered.is_some() {
        endpoint.oauth = offered;
        return Ok(endpoint);
    }
    let row = preset(&endpoint.name);
    if let Some(key) = row
        .filter(|row| own_base(&endpoint, row))
        .and_then(|row| row.default_key)
    {
        endpoint.api_key = Some(key.to_string());
        endpoint.source = Some(Source::Builtin);
        return Ok(endpoint);
    }
    if !require_key || endpoint.keyless() {
        return Ok(endpoint);
    }
    let missing = row
        .map(|row| row.missing)
        .filter(|text| !text.is_empty())
        .unwrap_or("run `summonr login NAME` or set api_key in ~/.summonr/config.toml");
    Err(Error::Config(format!(
        "{} has no key — {}",
        endpoint.name,
        missing.replace("NAME", &endpoint.name)
    )))
}

fn own_base(endpoint: &Endpoint, row: &super::preset::Preset) -> bool {
    endpoint
        .base_url
        .as_deref()
        .is_some_and(|base| trim_slash(base) == trim_slash(row.base_url))
}

fn peer_key(endpoint: &Endpoint, row: Option<&super::preset::Preset>) -> Option<(String, Source)> {
    let row = row.filter(|row| own_base(endpoint, row))?;
    if let Some(cli) = &row.cli_token
        && let Some(token) = (cli.read)()
    {
        return Some((token, Source::Peer(cli.label)));
    }
    let bearer = row.opencode_oauth && !matches!(endpoint.wire, Wire::Anthropic | Wire::Gemini);
    opencode_peer(provider_ids(row.name), bearer).map(|key| (key, Source::Peer("opencode")))
}

pub(crate) fn peer_token(name: &str) -> Option<String> {
    let file = config::file().ok()?;
    let endpoint = seat(&file, name)?;
    let row = preset(name).or_else(|| endpoint.base_url.as_deref().and_then(hosted));
    peer_key(&endpoint, row).map(|(key, _)| key)
}

fn opencode_peer(ids: &[&str], bearer: bool) -> Option<String> {
    let home = crate::config::home_dir()?;
    let path = home.join(".local/share/opencode/auth.json");
    let store: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    ids.iter().find_map(|id| {
        let entry = store.get(*id)?;
        let token = match entry.get("type").and_then(serde_json::Value::as_str) {
            Some("api") => entry.get("key"),
            Some("oauth") if bearer => {
                let expires = entry.get("expires").and_then(serde_json::Value::as_i64)?;
                let now = i64::try_from(crate::mail::now()).ok()?.saturating_mul(1000);
                if expires <= now + skew_millis() {
                    return None;
                }
                entry.get("access")
            }
            _other => return None,
        };
        token
            .and_then(serde_json::Value::as_str)
            .filter(|token| !token.trim().is_empty())
            .map(str::to_string)
    })
}

pub(super) fn builtin(name: &str) -> Option<Endpoint> {
    let row = preset(name)?;
    Some(Endpoint {
        name: row.name.into(),
        base_url: Some(row.base_url.into()),
        model: row.model.into(),
        api_key: None,
        source: None,
        oauth: row.oauth.map(|make| make()),
        body: None,
        context_window: row.context_window,
        idle_timeout_secs: None,
        effort: None,
        price: None,
        env: BTreeMap::new(),
        headers: row
            .headers
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect(),
        wire: row.wire,
        fallback: Vec::new(),
        compact_tokens: None,
        images: None,
        keep_reasoning: false,
        opencode: row.opencode,
        prompt_cache_key: row.prompt_cache_key,
        reasoning: reasoning::from_preset(Some(row)),
    })
}

pub(super) fn json(value: &toml::Value) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

pub(super) fn named_model(endpoint: &str, model: &str) -> String {
    let folded = model.trim().to_ascii_lowercase();
    if let Some(row) = preset(endpoint) {
        for (alias, canon) in row.aliases {
            if *alias == folded {
                return (*canon).to_string();
            }
        }
    }
    model.to_string()
}

pub(super) fn kimi_cli_token() -> Option<String> {
    let home = crate::config::home_dir()?;
    let path = home.join(".kimi-code/credentials/kimi-code.json");
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let expires = value
        .get("expires_at")
        .and_then(serde_json::Value::as_i64)?;
    let now = i64::try_from(crate::mail::now()).ok()?;
    if expires <= now + skew_secs() {
        return None;
    }
    value
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string)
}

pub(super) fn env_key(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

pub(super) fn trim_slash(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn skew_millis() -> i64 {
    i64::try_from(crate::limits::TOKEN_EXPIRY_SKEW.as_millis()).unwrap_or(i64::MAX)
}

fn skew_secs() -> i64 {
    i64::try_from(crate::limits::TOKEN_EXPIRY_SKEW.as_secs()).unwrap_or(i64::MAX)
}
