use std::sync::Arc;
use std::time::Duration;

use summonr_auth::{Auth, Grant, Method, Scheme, Spec, Store};

use crate::endpoint::{Endpoint, Wire};
use crate::error::{Error, Result};
use crate::secret;

const AUTH_TIMEOUT: Duration = Duration::from_secs(30);

struct Keeper;

impl Store for Keeper {
    fn get(&self, name: &str) -> summonr_auth::Result<Option<String>> {
        secret::get(name)
            .map(|found| found.map(|(secret, _)| secret))
            .map_err(|error| summonr_auth::Error::Config(error.to_string()))
    }

    fn set(&self, name: &str, secret: &str) -> summonr_auth::Result<()> {
        secret::set(name, secret)
            .map(|_source| ())
            .map_err(|error| summonr_auth::Error::Config(error.to_string()))
    }

    fn forget(&self, name: &str) -> summonr_auth::Result<bool> {
        secret::forget(name).map_err(|error| summonr_auth::Error::Config(error.to_string()))
    }
}

pub(crate) fn keeper() -> Arc<dyn Store> {
    Arc::new(Keeper)
}

pub(crate) fn scheme(wire: Wire) -> Scheme {
    match wire {
        Wire::Anthropic => Scheme::ApiKey,
        Wire::Gemini => Scheme::GoogleKey,
        Wire::Completions | Wire::Responses | Wire::Decisions => Scheme::Bearer,
    }
}

pub(crate) fn spec(row: &crate::config::OAuth) -> Result<Spec> {
    if row.token_url.trim().is_empty() || row.client_id.trim().is_empty() {
        return Err(Error::Config(
            "a login needs token_url and client_id".to_string(),
        ));
    }
    let grant = match row.grant.as_deref().map(str::trim) {
        Some("device_code") => Grant::DeviceCode,
        _ => Grant::RefreshToken,
    };
    let spec = Spec {
        token_url: row.token_url.clone(),
        client_id: row.client_id.clone(),
        client_secret: row.client_secret.clone(),
        refresh_token: row.refresh_token.clone(),
        scope: row.scope.clone(),
        grant,
        device_url: row.device_url.clone(),
        exchange_url: row.exchange_url.clone(),
    };
    spec.check()
        .map_err(|error| Error::Config(error.to_string()))?;
    Ok(spec)
}

pub(crate) fn auth(endpoint: &Endpoint) -> Result<Auth> {
    let method = match endpoint.oauth.as_ref() {
        Some(row) => Method::Login(spec(row)?),
        None => Method::Key(
            endpoint
                .api_key
                .clone()
                .filter(|key| !key.trim().is_empty())
                .unwrap_or_else(|| "local".to_string()),
        ),
    };
    Ok(Auth::new(
        method,
        scheme(endpoint.wire),
        &endpoint.name,
        keeper(),
    ))
}

pub(crate) fn mcp_auth(row: crate::config::OAuth, name: &str) -> Result<Auth> {
    Ok(Auth::new(
        Method::Login(spec(&row)?),
        Scheme::Bearer,
        name,
        keeper(),
    ))
}

pub(crate) fn bearer(key: impl Into<String>) -> Auth {
    Auth::new(Method::Key(key.into()), Scheme::Bearer, "local", keeper())
}

pub(crate) fn http() -> Result<leyline::Session> {
    summonr_auth::http(AUTH_TIMEOUT).map_err(|error| Error::Config(error.to_string()))
}

pub(crate) fn logged_in(row: &crate::config::OAuth, name: &str) -> Result<bool> {
    Auth::new(Method::Login(spec(row)?), Scheme::Bearer, name, keeper())
        .ready()
        .map_err(|error| Error::Config(error.to_string()))
}

pub(crate) fn is_device(row: &crate::config::OAuth) -> bool {
    row.grant.as_deref().map(str::trim) == Some("device_code")
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
