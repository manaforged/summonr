use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use leyline::{RequestBuilder, Session};
use serde_json::Value;

use crate::{Error, Issued, Parts, Result, Store};

use super::store::{expire, from_unix, keep, lifetime, save, stored};
use super::{Spec, TokenStyle};

const REFUSED: [u16; 2] = [401, 403];

enum Reply {
    Issued(Issued),
    Refused(String),
}

pub(super) async fn exchanged(
    parts: &Parts<'_>,
    http: &Session,
    shared: &Arc<dyn Store>,
    grant: Issued,
    claimed: bool,
) -> Result<String> {
    let spent = grant.access.clone();
    let chat = match exchange(parts.spec, http, grant).await? {
        Reply::Issued(chat) => chat,
        Reply::Refused(detail) => {
            let _claim = if claimed {
                None
            } else {
                Some(super::claim(parts, shared).await?)
            };
            return Err(revoked(parts, shared.as_ref(), &spent, detail));
        }
    };
    let access = chat.access.clone();
    keep(parts, chat)?;
    Ok(access)
}

fn revoked(parts: &Parts<'_>, store: &dyn Store, spent: &str, detail: String) -> Error {
    let current = match stored(parts, store) {
        Ok(current) => current,
        Err(error) => return error,
    };
    if let Some(grant) = current.filter(|grant| grant.access == spent)
        && let Err(error) = expire(parts, store, grant)
    {
        return error;
    }
    if parts.spec.grant.interactive() {
        crate::needs_login(
            parts.name,
            &format!("{detail}; run `summonr login {}`", parts.name),
        )
    } else {
        crate::refused(detail)
    }
}

pub(super) async fn settle(
    parts: &Parts<'_>,
    http: &Session,
    shared: &Arc<dyn Store>,
    grant: Issued,
) -> Result<()> {
    let claim = super::claim(parts, shared).await?;
    let saved = save(parts, shared.as_ref(), &grant);
    drop(claim);
    saved?;
    match exchange(parts.spec, http, grant).await? {
        Reply::Issued(chat) => keep(parts, chat),
        Reply::Refused(detail) => Err(crate::refused(detail)),
    }
}

async fn exchange(spec: &Spec, http: &Session, grant: Issued) -> Result<Reply> {
    let Some(url) = spec.exchange() else {
        return Ok(Reply::Issued(grant));
    };
    let (status, text) = sent(
        http.get(url)
            .header("user-agent", crate::AGENT)
            .header("Accept", "application/json")
            .bearer_auth(&grant.access),
    )
    .await?;
    let body = serde_json::from_str::<Value>(&text);
    if status >= 400 {
        let detail = match &body {
            Ok(body) => format!("token exchange HTTP {status}: {}", told(body)),
            Err(_) => format!("token exchange HTTP {status}"),
        };
        return if REFUSED.contains(&status) {
            Ok(Reply::Refused(detail))
        } else {
            Err(crate::refused(detail))
        };
    }
    let body = body.map_err(|error| crate::refused(error.to_string()))?;
    let mut chat = minted(&body)?;
    chat.refresh = grant.refresh;
    chat.identity = chat.identity.or(grant.identity);
    chat.client = grant.client;
    Ok(Reply::Issued(chat))
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
    let (status, text) = sent(request).await?;
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

async fn sent(request: RequestBuilder) -> Result<(u16, String)> {
    let response = request.send().await.map_err(crate::unreachable)?;
    let status = response.status().as_u16();
    let text = response.text().await.map_err(crate::unreachable)?;
    Ok((status, text))
}

pub(crate) fn minted(body: &Value) -> Result<Issued> {
    let access = body
        .get("access_token")
        .or_else(|| body.get("token"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            crate::refused(format!("the reply carried no access token: {}", told(body)))
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
        client: None,
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

pub(super) fn claim(token: &str, name: &str) -> Option<String> {
    payload(token).and_then(|body| dig(&body, name))
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
        .ok_or_else(|| crate::refused("no refresh token to spend".to_string()))?;
    let client = issued.client.as_deref().unwrap_or(&spec.client_id);
    let mut form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), token.to_string()),
        ("client_id".to_string(), client.to_string()),
    ];
    spec.extend_token_form(&mut form, true);
    let (status, body) = post(http, &spec.token_url, &form, spec.token_style).await?;
    if status >= 400 {
        return Err(crate::refused(format!(
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
    fresh.client = issued.client.clone();
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
