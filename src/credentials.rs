use std::sync::Arc;

use summonr_auth::{Auth, Grant, Method, Scheme, Spec, Store, TokenStyle};

use crate::endpoint::{Endpoint, LoginFlow, Wire};
use crate::error::{Error, Result};
use crate::secret;

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

    fn claim(&self, name: &str) -> summonr_auth::Result<Box<dyn Send>> {
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        crate::prompt::summonr_home()
            .and_then(|home| crate::config::hold(&home.join("locks").join(safe)))
            .map(|file| Box::new(file) as Box<dyn Send>)
            .map_err(|error| summonr_auth::Error::Config(error.to_string()))
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
        Some("authorization_code") => Grant::AuthorizationCode,
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
        authorize_url: row.authorize_url.clone(),
        redirect_uri: row.redirect_uri.clone(),
        authorize_params: row.authorize_params.clone(),
        token_params: row.token_params.clone(),
        token_style: match row.token_style.as_deref().map(str::trim) {
            Some("json") => TokenStyle::Json,
            _ => TokenStyle::Form,
        },
        pkce: row.pkce,
        claim_headers: row.claim_headers.clone(),
    };
    spec.check()
        .map_err(|error| Error::Config(error.to_string()))?;
    Ok(spec)
}

pub(crate) fn auth(endpoint: &Endpoint) -> Result<Auth> {
    let method = match endpoint.oauth.as_ref() {
        Some(row) => Method::Login(Box::new(spec(row)?)),
        None => match endpoint
            .api_key
            .clone()
            .filter(|key| !key.trim().is_empty())
        {
            Some(_) if matches!(endpoint.source, Some(secret::Source::Peer(_))) => {
                Method::Peer(crate::endpoint::peer_token, endpoint.name.clone())
            }
            Some(key) => Method::Key(key),
            None if endpoint.keyless() => Method::Key("local".to_string()),
            None => Method::Key(String::new()),
        },
    };
    let carrier = match (&method, endpoint.oauth.as_ref().and_then(named_scheme)) {
        (Method::Login(_), Some(named)) => named,
        (Method::Login(_), None) => Scheme::Bearer,
        (Method::Key(_) | Method::Peer(..), _) => scheme(endpoint.wire),
    };
    Ok(Auth::new(method, carrier, &endpoint.name, keeper()))
}

pub(crate) fn mcp_auth(row: crate::config::OAuth, name: &str) -> Result<Auth> {
    Ok(Auth::new(
        Method::Login(Box::new(spec(&row)?)),
        Scheme::Bearer,
        name,
        keeper(),
    ))
}

pub(crate) fn bearer(key: impl Into<String>) -> Auth {
    Auth::new(Method::Key(key.into()), Scheme::Bearer, "local", keeper())
}

pub(crate) fn http() -> Result<leyline::Session> {
    session(summonr_auth::builder(
        crate::limits::AUTH,
        crate::limits::CONNECT,
    ))
}

pub(crate) fn session(builder: leyline::SessionBuilder) -> Result<leyline::Session> {
    builder
        .retry(crate::limits::retry_policy())
        .build()
        .map_err(|error| Error::Config(error.to_string()))
}

pub(crate) fn logged_in(row: &crate::config::OAuth, name: &str) -> Result<bool> {
    Auth::new(
        Method::Login(Box::new(spec(row)?)),
        Scheme::Bearer,
        name,
        keeper(),
    )
    .ready()
    .map_err(|error| Error::Config(error.to_string()))
}

fn named_scheme(row: &crate::config::OAuth) -> Option<Scheme> {
    match row.scheme.as_deref().map(str::trim) {
        Some("api_key") => Some(Scheme::ApiKey),
        Some("google_key") => Some(Scheme::GoogleKey),
        Some("bearer") => Some(Scheme::Bearer),
        _ => None,
    }
}

pub(crate) fn flow(row: &crate::config::OAuth) -> LoginFlow {
    match row.grant.as_deref().map(str::trim) {
        Some("device_code") => LoginFlow::Device,
        Some("authorization_code") => LoginFlow::Browser,
        _ => LoginFlow::Key,
    }
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
