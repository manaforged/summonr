use std::sync::Arc;

use summonr_auth::{Auth, Method, Scheme, Spec, Store};
use summonr_core::secret::Secret;
use summonr_types::oauth::Grant;

use crate::endpoint::{Endpoint, LoginFlow, Wire};
use crate::secret;
use summonr_core::error::{Error, ErrorKind, Result};

pub(crate) fn login_slot(name: &str) -> String {
    format!("oauth:{name}")
}

impl Store for secret::Store {
    fn get(&self, name: &str) -> Result<Option<String>> {
        secret::Store::get(self, &login_slot(name)).map(|found| found.map(|(secret, _)| secret))
    }

    fn set(&self, name: &str, secret: &str) -> Result<()> {
        secret::Store::set(self, &login_slot(name), secret).map(|_source| ())
    }

    fn forget(&self, name: &str) -> Result<bool> {
        secret::Store::forget(self, &login_slot(name))
    }

    fn claim(&self, name: &str) -> Result<Box<dyn Send>> {
        let safe = summonr_core::text::clean_name(
            &login_slot(name),
            summonr_core::text::Charset::Dash,
            '_',
            usize::MAX,
        );
        summonr_core::dirs::summonr_home()
            .and_then(|home| crate::config::hold(&home.join("locks").join(safe)))
            .map(|file| Box::new(file) as Box<dyn Send>)
    }
}

fn store() -> Result<Arc<dyn Store>> {
    Ok(Arc::new(secret::Store::discover()?))
}

pub(crate) fn scheme(wire: Wire) -> Scheme {
    crate::provider::wire::spec(wire).scheme
}

pub(crate) fn spec(row: &crate::config::OAuth) -> Result<Spec> {
    Spec::try_from(row)
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
        (Method::Login(_), Some(named)) => Scheme::from(named),
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

pub fn http(total: std::time::Duration) -> Result<leyline::Session> {
    session(
        summonr_auth::builder(total, crate::limits::limits().connect()),
        crate::limits::limits().retry_policy(crate::limits::Reach::Remote),
    )
}

/// GETs `url` and returns its body, or why it failed; with `cap`, refuses a body of `cap` bytes or more.
pub async fn download(
    http: &leyline::Session,
    url: &str,
    cap: Option<usize>,
) -> std::result::Result<Vec<u8>, String> {
    let failed = |why: String| format!("cannot download {url}: {why}");
    let request = http.get(url);
    let request = if cap.is_some() {
        request.stream()
    } else {
        request
    };
    let response = request
        .send()
        .await
        .map_err(|error| failed(error.to_string()))?;
    if !response.status().is_success() {
        return Err(failed(format!("HTTP {}", response.status().as_u16())));
    }
    let Some(cap) = cap else {
        return response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| failed(error.to_string()));
    };
    let (bytes, _) = response
        .read_until(cap, |_, _| false)
        .await
        .map_err(|error| failed(error.to_string()))?;
    if bytes.len() >= cap {
        return Err(failed(format!("larger than {cap} bytes")));
    }
    Ok(bytes)
}

pub(crate) fn session(
    builder: leyline::SessionBuilder,
    retry: leyline::Retry,
) -> Result<leyline::Session> {
    builder
        .retry(retry)
        .build()
        .map_err(|error| Error::caused(ErrorKind::Config, error))
}

pub(crate) fn logged_in(row: &crate::config::OAuth, name: &str) -> Result<bool> {
    Auth::new(
        Method::Login(Box::new(spec(row)?)),
        Scheme::Bearer,
        name,
        store()?,
    )
    .ready()
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
