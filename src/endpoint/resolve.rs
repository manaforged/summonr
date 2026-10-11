use super::preset::presets;
use super::*;

mod pick;
pub(crate) use pick::{choose, default_seat};
use pick::{first_ready, last_used};
pub use pick::{ordered_names, set_up};

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

pub fn resolve(name: Option<&str>, model: Option<&str>, need: Need) -> Result<Endpoint> {
    if let Ok(base) = std::env::var("SUMMONR_BASE_URL")
        && !base.trim().is_empty()
        && matches!(name, None | Some(ENV_ENDPOINT))
    {
        return Ok(env_endpoint(&base, model));
    }

    let file = config::file()?;
    let (chosen, model_flag, _) = choose(&file, last_used(), name, model);

    let chosen = match chosen {
        Some(chosen) => chosen,
        None => unchosen(&file, need)?,
    };
    let mut endpoint = seat(&file, &chosen).ok_or_else(|| unknown(&chosen))?;

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
        endpoint.overlay_model(spec);
    }

    fill_key(endpoint, need.key())
}

fn env_endpoint(base: &str, model: Option<&str>) -> Endpoint {
    Endpoint {
        name: ENV_ENDPOINT.to_string(),
        base_url: Some(trim_slash(base)),
        models_url: None,
        model: model
            .map(str::to_string)
            .or_else(|| std::env::var("SUMMONR_MODEL").ok())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| super::preset::env_default().map(|preset| preset.model.to_string()))
            .unwrap_or_default(),
        api_key: env_key(&["SUMMONR_API_KEY"])
            .or_else(|| super::preset::env_default().and_then(|preset| env_key(preset.env_keys))),
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
        opencode: hosted(base).is_some_and(|preset| preset.opencode),
        prompt_cache_key: hosted(base).is_some_and(|preset| preset.prompt_cache_key),
        assistant_after_tool: hosted(base).is_some_and(|preset| preset.assistant_after_tool),
        cache_ttl: hosted(base)
            .and_then(|preset| preset.cache_ttl)
            .unwrap_or(crate::limits::limits().cache_ttl),
        cache_header: hosted(base).and_then(|preset| preset.cache_header.clone()),
        reasoning: reasoning::from_preset(hosted(base)),
    }
}

fn unchosen(file: &File, need: Need) -> Result<String> {
    match first_ready(file) {
        Some(name) => Ok(name),
        None if need.key() => Err(Error::Setup(no_seat())),
        None => Ok(presets()
            .next()
            .map_or_else(String::new, |row| row.name.to_string())),
    }
}

fn unknown(chosen: &str) -> Error {
    let names = names().unwrap_or_default();
    let hint = closest(chosen, &names, Near::Typo)
        .map_or_else(String::new, |name| format!("did you mean `{name}`? "));
    Error::Config(format!(
        "unknown endpoint `{chosen}`: {hint}`summonr endpoints` lists them"
    ))
}

fn no_seat() -> String {
    let config =
        config::path().map_or_else(|_| "config.toml".to_string(), |path| config::tilde(&path));
    format!(
        "no provider is set up yet.\n  Fix: `summonr login NAME`, or `export ANTHROPIC_API_KEY=…`, or a local [endpoint.NAME] base_url in {config}.\n  `summonr login` lists every provider and its status."
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Near {
    Relative,
    Typo,
}

#[must_use]
pub fn closest<'a>(name: &str, names: &'a [String], near: Near) -> Option<&'a str> {
    let length = name.chars().count();
    let typo = near == Near::Typo && length >= crate::limits::limits().typo_min_chars;
    names
        .iter()
        .map(|other| (cost(name, other), other.as_str()))
        .filter(|(cost, _)| {
            cost.saturating_mul(100)
                <= length.saturating_mul(crate::limits::limits().closest_percent)
                || (typo && *cost <= 1)
        })
        .min()
        .map(|(_, other)| other)
}

fn cost(name: &str, other: &str) -> usize {
    let left: Vec<char> = name.chars().collect();
    let right: Vec<char> = other.chars().collect();
    let mut rows = vec![(0..=right.len()).collect::<Vec<usize>>()];
    for (index, letter) in left.iter().enumerate() {
        let mut row = vec![index + 1];
        for (column, mark) in right.iter().enumerate() {
            let above = &rows[index];
            let mut next = (above[column] + usize::from(letter != mark))
                .min(above[column + 1] + 1)
                .min(row[column] + 1);
            if index > 0 && column > 0 && *letter == right[column - 1] && left[index - 1] == *mark {
                next = next.min(rows[index - 1][column - 1] + 1);
            }
            row.push(next);
        }
        rows.push(row);
    }
    rows[left.len()][right.len()]
}

pub(super) fn seat(file: &File, chosen: &str) -> Option<Endpoint> {
    let mut endpoint = builtin(chosen).or_else(|| from_file(file, chosen))?;
    if let Some(row) = file.endpoint.get(chosen) {
        endpoint.overlay(row);
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
        models_url: row.models_url.clone(),
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
        assistant_after_tool: preset.is_some_and(|preset| preset.assistant_after_tool),
        cache_ttl: row
            .cache_ttl
            .or_else(|| preset.and_then(|preset| preset.cache_ttl))
            .unwrap_or(crate::limits::limits().cache_ttl),
        cache_header: preset.and_then(|preset| preset.cache_header.clone()),
        reasoning: reasoning::from_preset(preset),
        wire: row.wire.unwrap_or(Wire::Completions),
    })
}

pub(super) fn hosted(base: &str) -> Option<&'static super::preset::Preset> {
    let base = trim_slash(base);
    presets()
        .filter(|preset| {
            base.strip_prefix(&trim_slash(&preset.base_url))
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

impl Endpoint {
    pub(super) fn overlay(&mut self, row: &FileEndpoint) {
        self.overlay_connection(row);
        self.overlay_tuning(row);
        reasoning::overlay(
            &mut self.reasoning,
            row.reasoning_levels.as_deref(),
            row.thinking_off,
        );
    }

    fn overlay_connection(&mut self, row: &FileEndpoint) {
        if let Some(base) = &row.base_url {
            self.base_url = Some(trim_slash(base));
        }
        if let Some(url) = &row.models_url {
            self.models_url = Some(trim_slash(url));
        }
        if let Some(model) = &row.model {
            self.model.clone_from(model);
        }
        if let Some(key) = &row.api_key
            && !key.trim().is_empty()
        {
            self.api_key = Some(key.clone());
        }
        if let Some(auth) = &row.auth {
            self.oauth = Some(auth.clone());
        }
        if let Some(body) = &row.body {
            self.body = Some(json(body));
        }
        for (key, value) in &row.env {
            self.env.insert(key.clone(), value.clone());
        }
        for (key, value) in &row.headers {
            self.headers.insert(key.clone(), value.clone());
        }
        if !row.fallback.is_empty() {
            self.fallback.clone_from(&row.fallback);
        }
    }

    fn overlay_tuning(&mut self, row: &FileEndpoint) {
        self.context_window = row.context_window.or(self.context_window);
        self.effort = row.effort.or(self.effort);
        self.price = row.price.or(self.price);
        self.wire = row.wire.unwrap_or(self.wire);
        self.compact_tokens = row.compact_tokens.or(self.compact_tokens);
        self.images = row.images.or(self.images);
        self.keep_reasoning = row.keep_reasoning.unwrap_or(self.keep_reasoning);
        self.cache_ttl = row.cache_ttl.unwrap_or(self.cache_ttl);
    }

    fn overlay_model(&mut self, spec: &crate::config::Model) {
        if let Some(body) = &spec.body {
            let body = json(body);
            match (&mut self.body, body) {
                (Some(serde_json::Value::Object(existing)), serde_json::Value::Object(extra)) => {
                    for (key, value) in extra {
                        existing.insert(key, value);
                    }
                }
                (slot, body) => *slot = Some(body),
            }
        }
        if let Some(window) = spec.context_window {
            self.context_window = Some(window);
        }
        if let Some(effort) = spec.effort {
            self.effort = Some(effort);
        }
        if let Some(tokens) = spec.compact_tokens {
            self.compact_tokens = Some(tokens);
        }
        if let Some(keep) = spec.keep_reasoning {
            self.keep_reasoning = keep;
        }
        reasoning::overlay(
            &mut self.reasoning,
            spec.reasoning_levels.as_deref(),
            spec.thinking_off,
        );
    }
}
