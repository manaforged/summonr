use std::collections::BTreeSet;

use summonr_agent::secret::Source;
use summonr_agent::{AuthInfo, Client, EndpointState as State, Result, splash};

struct Row {
    name: String,
    model: String,
    host: String,
    status: String,
}

pub(super) async fn list(all: bool, live: bool) -> Result<()> {
    let ready: BTreeSet<String> = summonr_agent::endpoint_set_up().into_iter().collect();
    let names = summonr_agent::endpoint_names_ordered()?;
    let shown: Vec<&String> = names
        .iter()
        .filter(|name| all || ready.is_empty() || ready.contains(*name))
        .collect();
    let refresh = async {
        if live {
            summonr_agent::catalog::refresh_registry().await
        } else {
            Ok(())
        }
    };
    let (refreshed, rows) = tokio::join!(
        refresh,
        futures::future::join_all(
            shown
                .iter()
                .map(|name| row(name, ready.contains(*name), live)),
        )
    );
    if let Err(error) = refreshed {
        eprintln!("summonr: models.dev catalog not refreshed: {error}");
    }
    let width = |column: fn(&Row) -> &str| {
        rows.iter()
            .map(|row| column(row).chars().count())
            .max()
            .unwrap_or(0)
    };
    let name_w = width(|row| &row.name);
    let model_w = width(|row| &row.model);
    let host_w = width(|row| &row.host);
    for Row {
        name,
        model,
        host,
        status,
    } in &rows
    {
        println!(
            "{}",
            format!("{name:<name_w$}  {model:<model_w$}  {host:<host_w$}  {status}").trim_end()
        );
    }
    let hidden = names.len() - shown.len();
    if hidden > 0 {
        eprintln!("{hidden} more not set up · summonr endpoint list --all");
    }
    Ok(())
}

async fn row(name: &str, set_up: bool, live: bool) -> Row {
    let info = summonr_agent::auth_info(name).ok();
    let model = info
        .as_ref()
        .map_or_else(String::new, |info| info.model.clone());
    let host = info
        .as_ref()
        .and_then(|info| info.base_url.as_deref())
        .map_or_else(String::new, |base| {
            splash::host_of(Some(base)).unwrap_or(base).to_string()
        });
    let status = match &info {
        None => State::Error.to_string(),
        Some(info) if info.unmodeled() => format!(
            "{} · run `summonr {name} --model NAME` or set model in config.toml",
            State::NeedsModel
        ),
        Some(info) if !set_up && !info.keyless() => {
            format!("{} · {}", info.state(), info.setup)
        }
        Some(info) => {
            let listing = summonr_agent::catalog::listing(&info.name);
            if live || (info.own && listing.is_none()) {
                check(name, info).await
            } else {
                let models = listing.map_or_else(String::new, |listing| {
                    format!(
                        " · {}{}",
                        count(listing.models.len()),
                        if listing.stale() { ", stale" } else { "" }
                    )
                });
                format!("{}{}{models}", State::Ready, key(info))
            }
        }
    };
    Row {
        name: name.to_string(),
        model,
        host,
        status,
    }
}

async fn check(name: &str, info: &AuthInfo) -> String {
    match summonr_agent::resolve_endpoint(Some(name), None, summonr_agent::Requirement::ModelAndKey)
    {
        Err(error) => format!("{} · {error}", State::Error),
        Ok(endpoint) if endpoint.base_url.is_none() => "adapter".to_string(),
        Ok(endpoint) => match Client::discover(&endpoint).await {
            Ok(models) => format!("{}{} · {}", State::Ready, key(info), count(models.len())),
            Err(error) => format!("{} · {error}{}", Client::state(&error), key(info)),
        },
    }
}

fn key(info: &AuthInfo) -> String {
    match info.source {
        _ if info.flow.signs_in() => " · signed in".to_string(),
        Some(Source::Env) => format!(" · key: {}", info.env.unwrap_or("env")),
        Some(Source::Builtin) | None if info.keyless() => " · no key needed".to_string(),
        Some(Source::Builtin) | None => String::new(),
        Some(source) => format!(" · key: {}", source.label()),
    }
}

fn count(models: usize) -> String {
    format!("{models} model{}", if models == 1 { "" } else { "s" })
}

pub(super) async fn usage(json: bool) -> Result<()> {
    let readings = summonr_agent::quota::read().await;
    if json {
        println!("{}", serde_json::to_string(&readings).map_err(|error| summonr_agent::Error::Config(error.to_string()))?);
        return Ok(());
    }
    if readings.is_empty() {
        println!("no plan with a usage source is set up · summonr endpoint list --all");
    }
    for reading in &readings {
        let via = reading.endpoint.as_deref().map_or_else(String::new, |name| format!(" ({name})"));
        println!("{}{via}", reading.plan);
        if let Some(error) = &reading.error {
            println!("  {error}");
        }
        for window in &reading.windows {
            let group = window.group.as_deref().map_or_else(String::new, |group| format!("{group} · "));
            let resets = window.resets.as_deref().map_or_else(String::new, |at| format!("  resets {}", local(at)));
            println!("  {group}{:<7} {:>3.0}% used{resets}", window.label(), window.used);
        }
    }
    Ok(())
}

fn local(at: &str) -> String {
    at.parse::<jiff::Timestamp>().map_or_else(
        |_| at.to_string(),
        |time| time.to_zoned(jiff::tz::TimeZone::system()).strftime("%a %-I:%M %p").to_string(),
    )
}
