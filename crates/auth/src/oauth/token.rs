use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use leyline::Session;
use serde_json::Value;

use crate::{Error, Issued, Parts, Result};

use super::store::{from_unix, keep, lifetime};
use super::{Spec, TokenStyle};

pub(super) async fn exchanged(parts: &Parts<'_>, http: &Session, grant: Issued) -> Result<String> {
    let chat = exchange(parts.spec, http, grant).await?;
    let access = chat.access.clone();
    keep(parts, chat)?;
    Ok(access)
}

pub(super) async fn exchange(spec: &Spec, http: &Session, grant: Issued) -> Result<Issued> {
    let Some(url) = spec
        .exchange_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    else {
        return Ok(grant);
    };
    let response = http
        .get(url)
        .header("user-agent", crate::AGENT)
        .header("Accept", "application/json")
        .bearer_auth(&grant.access)
        .send()
        .await
        .map_err(|error| Error::Transport(error.to_string()))?;
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|error| Error::Transport(error.to_string()))?;
    if status >= 400 {
        return Err(Error::Denied(format!(
            "token exchange HTTP {status}: {}",
            text.trim()
        )));
    }
    let body: Value =
        serde_json::from_str(&text).map_err(|error| Error::Denied(error.to_string()))?;
    let mut chat = minted(&body)?;
    chat.refresh = grant.refresh;
    chat.identity = chat.identity.or(grant.identity);
    Ok(chat)
}

pub(super) async fn post(
    http: &Session,
    url: &str,
    form: &[(String, String)],
    style: TokenStyle,
) -> Result<(u16, Value)> {
    let request = http
        .post(url)
        .header("user-agent", crate::AGENT)
        .header("Accept", "application/json");
    let request = match style {
        TokenStyle::Form => request.form(
            form.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        ),
        TokenStyle::Json => request.json(&Value::Object(
            form.iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect(),
        )),
    };
    let response = request
        .send()
        .await
        .map_err(|error| Error::Transport(error.to_string()))?;
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|error| Error::Transport(error.to_string()))?;
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

pub(crate) fn minted(body: &Value) -> Result<Issued> {
    let access = body
        .get("access_token")
        .or_else(|| body.get("token"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::Denied(format!("the reply carried no access token: {}", told(body)))
        })?;
    let expires = body
        .get("expires_in")
        .and_then(Value::as_u64)
        .map(lifetime)
        .or_else(|| {
            body.get("expires_at")
                .and_then(Value::as_u64)
                .map(from_unix)
        });
    Ok(Issued {
        access: access.to_string(),
        refresh: body
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_string),
        expires,
        identity: body
            .get("id_token")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub(crate) fn stamped(spec: &Spec, issued: &Issued) -> Vec<(String, String)> {
    let carried = [issued.identity.as_deref(), Some(issued.access.as_str())]
        .into_iter()
        .flatten()
        .filter_map(payload)
        .collect::<Vec<_>>();
    spec.claim_headers
        .iter()
        .filter_map(|(header, claim)| {
            carried
                .iter()
                .find_map(|body| dig(body, claim))
                .map(|found| (header.clone(), found))
        })
        .collect()
}

fn payload(token: &str) -> Option<Value> {
    let middle = token.split('.').nth(1)?;
    let raw = URL_SAFE_NO_PAD.decode(middle).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn dig(body: &Value, claim: &str) -> Option<String> {
    match body {
        Value::Object(fields) => fields.iter().find_map(|(key, value)| {
            if key == claim {
                match value {
                    Value::String(found) => Some(found.clone()),
                    Value::Number(found) => Some(found.to_string()),
                    _ => None,
                }
            } else {
                dig(value, claim)
            }
        }),
        Value::Array(items) => items.iter().find_map(|item| dig(item, claim)),
        _ => None,
    }
}

pub(super) async fn refresh(spec: &Spec, http: &Session, issued: &Issued) -> Result<Issued> {
    let token = issued
        .refresh
        .as_deref()
        .ok_or_else(|| Error::Denied("no refresh token to spend".to_string()))?;
    let mut form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), token.to_string()),
        ("client_id".to_string(), spec.client_id.clone()),
    ];
    if let Some(secret) = &spec.client_secret {
        form.push(("client_secret".to_string(), secret.clone()));
    }
    let (status, body) = post(http, &spec.token_url, &form, spec.token_style).await?;
    if status >= 400 {
        return Err(Error::Denied(format!(
            "refresh failed: HTTP {status} {}",
            told(&body)
        )));
    }
    let mut fresh = minted(&body)?;
    if fresh.refresh.is_none() {
        fresh.refresh = Some(token.to_string());
    }
    if fresh.identity.is_none() {
        fresh.identity = issued.identity.clone();
    }
    Ok(fresh)
}

pub(super) fn told(body: &Value) -> String {
    let said = ["error", "error_description"]
        .iter()
        .filter_map(|key| body.get(key).and_then(Value::as_str))
        .collect::<Vec<_>>();
    if !said.is_empty() {
        return said.join(": ");
    }
    let keys = body
        .as_object()
        .map(|fields| fields.keys().map(String::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    format!("fields [{}]", keys.join(", "))
}
