use std::sync::Arc;

use summonr_auth::{Auth, Method, Scheme, Spec, Store};
use summonr_core::secret::Secret;
use summonr_types::oauth::{Grant, TokenScheme};

use crate::endpoint::{Endpoint, LoginFlow, Wire};
use crate::secret;
use summonr_core::error::{Error, Result};

pub(crate) fn login_slot(name: &str) -> String {
    format!("oauth:{name}")
}

impl Store for secret::Store {
    fn get(&self, name: &str) -> summonr_auth::Result<Option<String>> {
        secret::Store::get(self, &login_slot(name))
            .map(|found| found.map(|(secret, _)| secret))
            .map_err(|error| summonr_auth::Error::Config(error.to_string()))
    }

    fn set(&self, name: &str, secret: &str) -> summonr_auth::Result<()> {
        secret::Store::set(self, &login_slot(name), secret)
            .map(|_source| ())
            .map_err(|error| summonr_auth::Error::Config(error.to_string()))
    }

    fn forget(&self, name: &str) -> summonr_auth::Result<bool> {
        secret::Store::forget(self, &login_slot(name))
            .map_err(|error| summonr_auth::Error::Config(error.to_string()))
    }

    fn claim(&self, name: &str) -> summonr_auth::Result<Box<dyn Send>> {
        let safe: String = login_slot(name)
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        summonr_core::dirs::summonr_home()
            .and_then(|home| crate::config::hold(&home.join("locks").join(safe)))
            .map(|file| Box::new(file) as Box<dyn Send>)
            .map_err(|error| summonr_auth::Error::Config(error.to_string()))
    }
}

fn store() -> Result<Arc<dyn Store>> {
    Ok(Arc::new(secret::Store::discover()?))
}

pub(crate) fn scheme(wire: Wire) -> Scheme {
    crate::provider::wire::spec(wire).scheme
}

pub(crate) fn spec(row: &crate::config::OAuth) -> Result<Spec> {
    if row.token_url.trim().is_empty() || row.client_id.trim().is_empty() {
        return Err(Error::Config(
            "a login needs token_url and client_id".to_string(),
        ));
    }
    let spec = Spec {
        token_url: row.token_url.clone(),
        client_id: row.client_id.clone(),
        client_secret: row.client_secret.clone(),
        refresh_token: row.refresh_token.clone(),
        scope: row.scope.clone(),
        grant: row.grant(),
        device_url: row.device_url.clone(),
        exchange_url: row.exchange_url.clone(),
        authorize_url: row.authorize_url.clone(),
        redirect_uri: row.redirect_uri.clone(),
        authorize_params: row.authorize_params.clone(),
        token_params: row.token_params.clone(),
        token_style: row.token_style(),
        pkce: row.pkce,
        claim_headers: row.claim_headers.clone(),
        required_scope: row.required_scope.clone(),
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
            .filter(|key| !key.expose().trim().is_empty())
        {
            Some(key) => Method::Key(key),
            None if endpoint.keyless() => Method::Key(Secret::new("local")),
            None => Method::Key(Secret::new("")),
        },
    };
    let carrier = match (
        &method,
        endpoint.oauth.as_ref().and_then(|row| row.scheme()),
    ) {
        (Method::Login(_), Some(named)) => carrier(named),
        (Method::Login(_), None) => Scheme::Bearer,
        (Method::Key(_), _) => scheme(endpoint.wire),
    };
    Ok(Auth::new(method, carrier, &endpoint.name, store()?))
}

pub(crate) fn mcp_auth(row: crate::config::OAuth, name: &str) -> Result<Auth> {
    Ok(Auth::new(
        Method::Login(Box::new(spec(&row)?)),
        Scheme::Bearer,
        name,
        store()?,
    ))
}

pub(crate) fn bearer(key: impl Into<String>) -> Result<Auth> {
    Ok(Auth::new(
        Method::Key(Secret::new(key)),
        Scheme::Bearer,
        "local",
        store()?,
    ))
}

pub(crate) fn http(total: std::time::Duration) -> Result<leyline::Session> {
    session(
        summonr_auth::builder(total, crate::limits::limits().connect()),
        crate::limits::limits().retry_policy(crate::limits::Reach::Remote),
    )
}

pub(crate) fn session(
    builder: leyline::SessionBuilder,
    retry: leyline::RetryPolicy,
) -> Result<leyline::Session> {
    builder
        .retry(retry)
        .build()
        .map_err(|error| Error::Config(error.to_string()))
}

pub(crate) fn logged_in(row: &crate::config::OAuth, name: &str) -> Result<bool> {
    Auth::new(
        Method::Login(Box::new(spec(row)?)),
        Scheme::Bearer,
        name,
        store()?,
    )
    .ready()
    .map_err(|error| Error::Config(error.to_string()))
}

fn carrier(named: TokenScheme) -> Scheme {
    match named {
        TokenScheme::ApiKey => Scheme::ApiKey,
        TokenScheme::GoogleKey => Scheme::GoogleKey,
        TokenScheme::Bearer => Scheme::Bearer,
    }
}

pub(crate) fn flow(row: &crate::config::OAuth) -> LoginFlow {
    match row.grant() {
        Grant::DeviceCode => LoginFlow::Device,
        Grant::AuthorizationCode => LoginFlow::Browser,
        Grant::RefreshToken => LoginFlow::Key,
    }
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
