use super::preset::presets;
use super::*;

pub fn fallbacks<P>(
    endpoint: &Endpoint,
    mut connect: impl FnMut(&Endpoint) -> Result<P>,
) -> (Vec<(P, String)>, Vec<String>) {
    let mut ready = Vec::new();
    let mut skipped = Vec::new();
    for name in &endpoint.fallback {
        match resolve(Some(name), None, Need::Seat)
            .and_then(|fallback| Ok((connect(&fallback)?, fallback.model)))
        {
            Ok(pair) => ready.push(pair),
            Err(error) => skipped.push(format!("fallback {name} skipped: {error}")),
        }
    }
    (ready, skipped)
}

pub fn vision() -> Result<Option<Endpoint>> {
    let file = config::file()?;
    let mut names: Vec<&str> = file
        .endpoint
        .iter()
        .filter(|(_, row)| row.images == Some(true))
        .map(|(name, _)| name.as_str())
        .collect();
    names.sort_unstable();
    Ok(names
        .into_iter()
        .find_map(|name| resolve(Some(name), None, Need::Seat).ok()))
}

pub fn chosen_by(name: Option<&str>, model: Option<&str>) -> Option<Pick> {
    let env = std::env::var("SUMMONR_BASE_URL").is_ok_and(|base| !base.trim().is_empty());
    if env && name.is_none() {
        return Some(Pick::Env);
    }
    let file = config::file().ok()?;
    choose(&file, last_used(), name, model).2
}

fn last_used() -> Option<(String, String)> {
    let home = crate::prompt::summonr_home().ok()?;
    let seat = crate::recent::load(&home).into_iter().next()?;
    Some((seat.endpoint, seat.model))
}

pub(crate) fn default_seat() -> Option<crate::recent::Seat> {
    let file = config::file().ok()?;
    Some(crate::recent::Seat::new(
        file.default.clone()?,
        file.default_model.clone()?,
    ))
}

pub(crate) fn choose(
    file: &File,
    last: Option<(String, String)>,
    name: Option<&str>,
    model: Option<&str>,
) -> (Option<String>, Option<String>, Option<Pick>) {
    let given = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let env = |key: &str| given(std::env::var(key).ok().as_deref());
    let (chosen, remembered, reason) = if let Some(name) = given(name) {
        (Some(name), None, Some(Pick::Flag))
    } else if let Some(name) = env("SUMMONR_ENDPOINT") {
        (Some(name), None, Some(Pick::Env))
    } else if let Some((name, model)) = last {
        (Some(name), Some(model), Some(Pick::LastUsed))
    } else if let Some(name) = file.default.clone() {
        (Some(name), file.default_model.clone(), Some(Pick::Default))
    } else {
        (file.last_endpoint.clone(), None, Some(Pick::LastUsed))
    };
    let reason = if given(model).is_some() {
        Some(Pick::ModelFlag)
    } else if env("SUMMONR_MODEL").is_some() {
        Some(Pick::Env)
    } else {
        reason.filter(|_| chosen.is_some())
    };
    let model = given(model).or_else(|| env("SUMMONR_MODEL")).or(remembered);
    (chosen, model, reason)
}

pub fn resolve(name: Option<&str>, model: Option<&str>, need: Need) -> Result<Endpoint> {
    if let Ok(base) = std::env::var("SUMMONR_BASE_URL")
        && !base.trim().is_empty()
        && matches!(name, None | Some("env"))
    {
        return Ok(Endpoint {
            name: "env".to_string(),
            base_url: Some(trim_slash(&base)),
            model: model
                .map(str::to_string)
                .or_else(|| std::env::var("SUMMONR_MODEL").ok())
                .filter(|value| !value.trim().is_empty())
                .or_else(|| super::preset::env_default().map(|preset| preset.model.to_string()))
                .unwrap_or_default(),
            api_key: env_key(&["SUMMONR_API_KEY"]).or_else(|| {
                super::preset::env_default().and_then(|preset| env_key(preset.env_keys))
            }),
            source: None,
            oauth: None,
            body: std::env::var("SUMMONR_MAX_TOKENS")
                .ok()
                .and_then(|value| value.trim().parse::<u32>().ok())
                .map(|tokens| serde_json::json!({ "max_tokens": tokens })),
            context_window: None,
            idle_timeout_secs: None,
            effort: None,
            price: None,
            env: BTreeMap::new(),
            headers: BTreeMap::new(),
            wire: Wire::Completions,
            fallback: Vec::new(),
            compact_tokens: None,
            images: None,
            keep_reasoning: std::env::var_os("SUMMONR_KEEP_REASONING").is_some(),
            opencode: hosted(&base).is_some_and(|preset| preset.opencode),
            prompt_cache_key: hosted(&base).is_some_and(|preset| preset.prompt_cache_key),
            reasoning: reasoning::from_preset(hosted(&base)),
        });
    }

    let file = config::file()?;
    let (chosen, model_flag, _) = choose(&file, last_used(), name, model);

    let chosen = match chosen {
        Some(chosen) => chosen,
        None => match first_ready(&file) {
            Some(name) => name,
            None if need.key() => return Err(Error::Config(no_seat())),
            None => presets()
                .next()
                .map_or_else(String::new, |row| row.name.to_string()),
        },
    };
    let mut endpoint = seat(&file, &chosen).ok_or_else(|| {
        let names = names().unwrap_or_default();
        let hint = closest(&chosen, &names)
            .map_or_else(String::new, |name| format!("did you mean `{name}`? "));
        Error::Config(format!(
            "unknown endpoint `{chosen}` — {hint}`summonr endpoints` lists them"
        ))
    })?;

    if let Some(model) = model_flag {
        endpoint.model = named_model(&endpoint.name, &model);
    } else if need.model() && endpoint.model.is_empty() {
        return Err(Error::Config(format!(
            "endpoint `{}` needs --model (or model in config.toml)",
            endpoint.name
        )));
    } else {
        endpoint.model = named_model(&endpoint.name, &endpoint.model);
    }

    if let Some(spec) = file
        .endpoint
        .get(&endpoint.name)
        .and_then(|row| row.models.get(&endpoint.model))
    {
        overlay_model(&mut endpoint, spec);
    }

    fill_key(endpoint, need.key())
}

fn first_ready(file: &File) -> Option<String> {
    names().ok()?.into_iter().find(|name| {
        resolve(Some(name), None, Need::Seat).is_ok_and(|endpoint| {
            endpoint.wire != Wire::Decisions
                && (endpoint
                    .source
                    .is_some_and(|source| source != Source::Builtin)
                    || (file.endpoint.contains_key(name) && endpoint.keyless())
                    || endpoint
                        .oauth
                        .as_ref()
                        .is_some_and(|spec| logged_in(spec, name).is_ok_and(|ready| ready)))
        })
    })
}

fn no_seat() -> String {
    let config =
        config::path().map_or_else(|_| "config.toml".to_string(), |path| config::tilde(&path));
    format!(
        "no provider is set up yet.\n  Fix: `summonr login NAME`, or `export ANTHROPIC_API_KEY=…`, or a local [endpoint.NAME] base_url in {config}.\n  `summonr login` lists every provider and its status."
    )
}

#[must_use]
pub fn closest<'a>(name: &str, names: &'a [String]) -> Option<&'a str> {
    let distance = |other: &str| {
        let mut row: Vec<usize> = (0..=other.chars().count()).collect();
        for (index, left) in name.chars().enumerate() {
            let mut last = row[0];
            row[0] = index + 1;
            for (column, right) in other.chars().enumerate() {
                let next = (last + usize::from(left != right))
                    .min(row[column] + 1)
                    .min(row[column + 1] + 1);
                last = row[column + 1];
                row[column + 1] = next;
            }
        }
        row[row.len() - 1]
    };
    names
        .iter()
        .map(|other| (distance(other), other.as_str()))
        .filter(|(cost, _)| *cost <= (name.chars().count() / 3).max(2))
        .min()
        .map(|(_, other)| other)
}

pub(super) fn seat(file: &File, chosen: &str) -> Option<Endpoint> {
    let mut endpoint = builtin(chosen).or_else(|| from_file(file, chosen))?;
    if let Some(row) = file.endpoint.get(chosen) {
        overlay(&mut endpoint, row);
    }
    Some(endpoint)
}

pub(super) fn from_file(file: &File, chosen: &str) -> Option<Endpoint> {
    let row = file.endpoint.get(chosen)?;
    let preset = row.base_url.as_deref().and_then(hosted);
    Some(Endpoint {
        name: chosen.to_string(),
        model: row.model.clone().unwrap_or_default(),
        base_url: row.base_url.as_deref().map(trim_slash),
        api_key: row.api_key.clone().filter(|key| !key.trim().is_empty()),
        source: None,
        oauth: row.auth.clone(),
        body: row.body.as_ref().map(json),
        context_window: row.context_window,
        idle_timeout_secs: row.idle_timeout_secs,
        effort: row.effort,
        price: row.price,
        env: row.env.clone(),
        headers: row.headers.clone(),
        fallback: row.fallback.clone(),
        compact_tokens: row.compact_tokens,
        images: row.images,
        keep_reasoning: row.keep_reasoning.unwrap_or(false),
        opencode: preset.is_some_and(|preset| preset.opencode),
        prompt_cache_key: preset.is_some_and(|preset| preset.prompt_cache_key),
        reasoning: reasoning::from_preset(preset),
        wire: row.wire.unwrap_or(Wire::Completions),
    })
}

pub(super) fn hosted(base: &str) -> Option<&'static super::preset::Preset> {
    let base = trim_slash(base);
    presets()
        .filter(|preset| {
            base.strip_prefix(&trim_slash(preset.base_url))
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
        .reduce(|best, next| {
            if next.base_url.len() > best.base_url.len() {
                next
            } else {
                best
            }
        })
}

pub(super) fn overlay(endpoint: &mut Endpoint, row: &FileEndpoint) {
    overlay_connection(endpoint, row);
    overlay_tuning(endpoint, row);
    reasoning::overlay(
        &mut endpoint.reasoning,
        row.reasoning_levels.as_deref(),
        row.thinking_off,
    );
}

fn overlay_connection(endpoint: &mut Endpoint, row: &FileEndpoint) {
    if let Some(base) = &row.base_url {
        endpoint.base_url = Some(trim_slash(base));
    }
    if let Some(model) = &row.model {
        endpoint.model.clone_from(model);
    }
    if let Some(key) = &row.api_key
        && !key.trim().is_empty()
    {
        endpoint.api_key = Some(key.clone());
    }
    if let Some(auth) = &row.auth {
        endpoint.oauth = Some(auth.clone());
    }
    if let Some(body) = &row.body {
        endpoint.body = Some(json(body));
    }
    for (key, value) in &row.env {
        endpoint.env.insert(key.clone(), value.clone());
    }
    for (key, value) in &row.headers {
        endpoint.headers.insert(key.clone(), value.clone());
    }
    if !row.fallback.is_empty() {
        endpoint.fallback.clone_from(&row.fallback);
    }
}

fn overlay_tuning(endpoint: &mut Endpoint, row: &FileEndpoint) {
    endpoint.context_window = row.context_window.or(endpoint.context_window);
    endpoint.effort = row.effort.or(endpoint.effort);
    endpoint.price = row.price.or(endpoint.price);
    endpoint.wire = row.wire.unwrap_or(endpoint.wire);
    endpoint.compact_tokens = row.compact_tokens.or(endpoint.compact_tokens);
    endpoint.images = row.images.or(endpoint.images);
    endpoint.keep_reasoning = row.keep_reasoning.unwrap_or(endpoint.keep_reasoning);
}

fn overlay_model(endpoint: &mut Endpoint, spec: &crate::config::Model) {
    if let Some(body) = &spec.body {
        let body = json(body);
        match (&mut endpoint.body, body) {
            (Some(serde_json::Value::Object(existing)), serde_json::Value::Object(extra)) => {
                for (key, value) in extra {
                    existing.insert(key, value);
                }
            }
            (slot, body) => *slot = Some(body),
        }
    }
    if let Some(window) = spec.context_window {
        endpoint.context_window = Some(window);
    }
    if let Some(effort) = spec.effort {
        endpoint.effort = Some(effort);
    }
    if let Some(tokens) = spec.compact_tokens {
        endpoint.compact_tokens = Some(tokens);
    }
    if let Some(keep) = spec.keep_reasoning {
        endpoint.keep_reasoning = keep;
    }
    reasoning::overlay(
        &mut endpoint.reasoning,
        spec.reasoning_levels.as_deref(),
        spec.thinking_off,
    );
}
