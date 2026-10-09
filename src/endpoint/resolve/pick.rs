use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

static NOTED: AtomicBool = AtomicBool::new(false);

pub(super) fn last_used() -> Option<(String, String)> {
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

type Chosen = (Option<String>, Option<String>, Option<Pick>);

pub(crate) fn choose(
    file: &File,
    last: Option<(String, String)>,
    name: Option<&str>,
    model: Option<&str>,
) -> Chosen {
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
    } else {
        remembered(file, last)
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

fn remembered(file: &File, last: Option<(String, String)>) -> Chosen {
    let config =
        config::path().map_or_else(|_| "config.toml".to_string(), |path| config::tilde(&path));
    let candidates = [
        file.default
            .clone()
            .map(|name| (name, file.default_model.clone(), Pick::Default)),
        last.map(|(name, model)| (name, Some(model), Pick::LastUsed)),
    ];
    let mut notes = Vec::new();
    let mut found = (None, None, None);
    for (name, model, pick) in candidates.into_iter().flatten() {
        match usable(file, &name) {
            Ok(()) => {
                found = (Some(name), model, Some(pick));
                break;
            }
            Err(reason) => notes.push(match pick {
                Pick::Default => format!(
                    "summonr: `default = \"{name}\"` in {config} is skipped: {reason}. Fix that endpoint, or remove `default`"
                ),
                Pick::LastUsed | Pick::Flag | Pick::ModelFlag | Pick::Env => format!(
                    "summonr: the last used endpoint `{name}` is skipped: {reason}. Pick another with /model"
                ),
            }),
        }
    }
    if !notes.is_empty() && !NOTED.swap(true, Ordering::Relaxed) {
        for note in notes {
            eprintln!("{note}");
        }
    }
    found
}

fn usable(file: &File, name: &str) -> std::result::Result<(), String> {
    let text = |error: Error| match error {
        Error::Config(text) | Error::Setup(text) => text,
        other => other.to_string(),
    };
    let endpoint = seat(file, name).ok_or_else(|| text(unknown(name)))?;
    let endpoint = fill_key(endpoint, true).map_err(text)?;
    match &endpoint.oauth {
        Some(spec) if !logged_in(spec, name).is_ok_and(|ready| ready) => Err(format!(
            "{name} is not signed in; run `summonr login {name}`"
        )),
        Some(_) | None => Ok(()),
    }
}

fn set_up_in(file: &File, name: &str) -> bool {
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
}

pub(super) fn first_ready(file: &File) -> Option<String> {
    names().ok()?.into_iter().find(|name| set_up_in(file, name))
}

#[must_use]
pub fn set_up() -> Vec<String> {
    let Ok(file) = config::file() else {
        return Vec::new();
    };
    names()
        .unwrap_or_default()
        .into_iter()
        .filter(|name| set_up_in(&file, name))
        .collect()
}

pub fn ordered_names() -> Result<Vec<String>> {
    let ready = set_up();
    let (mut first, rest): (Vec<String>, Vec<String>) =
        names()?.into_iter().partition(|name| ready.contains(name));
    first.extend(rest);
    Ok(first)
}
