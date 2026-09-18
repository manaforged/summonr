use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use leyline::Session;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Issued, Parts, Result, Store};

const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const POLL_FLOOR: Duration = Duration::from_secs(1);
const POLL_CEILING: Duration = Duration::from_secs(30);
const LIFETIME_CEILING: u64 = 10 * 365 * 24 * 3600;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    #[default]
    RefreshToken,
    DeviceCode,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Spec {
    pub token_url: String,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub grant: Grant,
    #[serde(default)]
    pub device_url: Option<String>,
    #[serde(default)]
    pub exchange_url: Option<String>,
}

impl Spec {
    pub fn check(&self) -> Result<()> {
        if self.grant == Grant::DeviceCode && self.device_url.is_none() {
            return Err(Error::Config(
                "a device_code login needs device_url".to_string(),
            ));
        }
        if self.grant == Grant::RefreshToken && self.refresh_token.is_none() {
            return Err(Error::Config(
                "a refresh_token login needs refresh_token".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct Stored {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<u64>,
}

pub(crate) fn parse_stored(raw: &str) -> Option<Issued> {
    let disk: Stored = serde_json::from_str(raw).ok()?;
    Some(Issued {
        access: disk.access_token,
        refresh: disk.refresh_token,
        expires: disk.expires_at.map(from_unix),
    })
}

fn to_stored(issued: &Issued) -> Stored {
    Stored {
        access_token: issued.access.clone(),
        refresh_token: issued.refresh.clone(),
        expires_at: issued.expires.map(to_unix),
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

fn from_unix(at: u64) -> Instant {
    Instant::now() + Duration::from_secs(at.saturating_sub(now_unix()).min(LIFETIME_CEILING))
}

fn to_unix(at: Instant) -> u64 {
    now_unix() + at.saturating_duration_since(Instant::now()).as_secs()
}

fn lifetime(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds.min(LIFETIME_CEILING))
}

pub(crate) async fn token(parts: Parts<'_>, http: &Session, store: &dyn Store) -> Result<String> {
    if let Some(live) = held(&parts)? {
        return Ok(live.access);
    }
    if let Some(grant) = stored(&parts, store)? {
        if grant.fresh() {
            return exchanged(&parts, http, grant).await;
        }
        if grant.refresh.is_some() {
            match refresh(parts.spec, http, &grant).await {
                Ok(next) => {
                    save(&parts, store, next.clone())?;
                    return exchanged(&parts, http, next).await;
                }
                Err(error) => {
                    if let Some(again) = stored(&parts, store)?
                        && again.fresh()
                        && again.access != grant.access
                    {
                        return exchanged(&parts, http, again).await;
                    }
                    if parts.spec.grant == Grant::DeviceCode {
                        return Err(Error::NeedsLogin {
                            name: parts.name.to_string(),
                            detail: format!("refresh failed ({error})"),
                        });
                    }
                    return Err(error);
                }
            }
        }
    }
    if parts.spec.grant == Grant::DeviceCode {
        return Err(Error::NeedsLogin {
            name: parts.name.to_string(),
            detail: format!("run `summonr login {}`", parts.name),
        });
    }
    let seed = Issued {
        access: String::new(),
        refresh: parts.spec.refresh_token.clone(),
        expires: None,
    };
    let minted = refresh(parts.spec, http, &seed).await?;
    save(&parts, store, minted.clone())?;
    exchanged(&parts, http, minted).await
}

fn stored(parts: &Parts<'_>, store: &dyn Store) -> Result<Option<Issued>> {
    Ok(store.get(parts.name)?.as_deref().and_then(parse_stored))
}

async fn exchanged(parts: &Parts<'_>, http: &Session, grant: Issued) -> Result<String> {
    let chat = exchange(parts.spec, http, grant).await?;
    let access = chat.access.clone();
    keep(parts, chat)?;
    Ok(access)
}

async fn exchange(spec: &Spec, http: &Session, grant: Issued) -> Result<Issued> {
    let Some(url) = spec
        .exchange_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    else {
        return Ok(grant);
    };
    let mut response = http
        .get(url)
        .user_agent(crate::AGENT)
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
    Ok(chat)
}

fn held(parts: &Parts<'_>) -> Result<Option<Issued>> {
    let slot = parts
        .held
        .lock()
        .map_err(|_| Error::Config("the token cache was poisoned".to_string()))?;
    Ok(slot.as_ref().filter(|issued| issued.fresh()).cloned())
}

fn keep(parts: &Parts<'_>, issued: Issued) -> Result<()> {
    let mut slot = parts
        .held
        .lock()
        .map_err(|_| Error::Config("the token cache was poisoned".to_string()))?;
    slot.replace(issued);
    Ok(())
}

fn save(parts: &Parts<'_>, store: &dyn Store, issued: Issued) -> Result<()> {
    let raw = serde_json::to_string(&to_stored(&issued))
        .map_err(|error| Error::Config(error.to_string()))?;
    store.set(parts.name, &raw)?;
    keep(parts, issued)
}

async fn post_form(http: &Session, url: &str, form: &[(String, String)]) -> Result<(u16, Value)> {
    let mut response = http
        .post(url)
        .user_agent(crate::AGENT)
        .header("Accept", "application/json")
        .form(
            form.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
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

fn minted(body: &Value) -> Result<Issued> {
    let access = body
        .get("access_token")
        .or_else(|| body.get("token"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Denied(format!("the reply carried no access token: {body}")))?;
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
    })
}

async fn refresh(spec: &Spec, http: &Session, issued: &Issued) -> Result<Issued> {
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
    let (status, body) = post_form(http, &spec.token_url, &form).await?;
    if status >= 400 {
        return Err(Error::Denied(format!(
            "refresh failed: HTTP {status} {body}"
        )));
    }
    let mut fresh = minted(&body)?;
    if fresh.refresh.is_none() {
        fresh.refresh = Some(token.to_string());
    }
    Ok(fresh)
}

pub(crate) async fn login(
    parts: Parts<'_>,
    http: &Session,
    store: &dyn Store,
    mut notice: impl FnMut(&str),
) -> Result<()> {
    parts.spec.check()?;
    if parts.spec.grant == Grant::RefreshToken {
        let seed = Issued {
            access: String::new(),
            refresh: parts.spec.refresh_token.clone(),
            expires: None,
        };
        let minted = refresh(parts.spec, http, &seed).await?;
        return save(&parts, store, minted);
    }
    let device_url = parts
        .spec
        .device_url
        .as_deref()
        .ok_or_else(|| Error::Config("a device_code login needs device_url".to_string()))?;
    let mut form = vec![("client_id".to_string(), parts.spec.client_id.clone())];
    if let Some(scope) = &parts.spec.scope {
        form.push(("scope".to_string(), scope.clone()));
    }
    let (status, body) = post_form(http, device_url, &form).await?;
    if status >= 400 {
        return Err(Error::Denied(format!(
            "the device request failed: HTTP {status} {body}"
        )));
    }
    let code = body
        .get("device_code")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Denied(format!("no device_code in the reply: {body}")))?;
    let shown = body
        .get("user_code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let uri = body
        .get("verification_uri_complete")
        .or_else(|| body.get("verification_uri"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    notice(&format!("open {uri} and enter {shown}"));
    let mut wait = body
        .get("interval")
        .and_then(Value::as_u64)
        .map_or(Duration::from_secs(5), Duration::from_secs)
        .clamp(POLL_FLOOR, POLL_CEILING);
    let stop = Instant::now()
        + body
            .get("expires_in")
            .and_then(Value::as_u64)
            .map_or(Duration::from_secs(900), Duration::from_secs);
    while Instant::now() < stop {
        tokio::time::sleep(wait).await;
        let form = vec![
            ("grant_type".to_string(), DEVICE_GRANT.to_string()),
            ("device_code".to_string(), code.to_string()),
            ("client_id".to_string(), parts.spec.client_id.clone()),
        ];
        let (status, body) = post_form(http, &parts.spec.token_url, &form).await?;
        match body.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                wait = (wait + Duration::from_secs(5)).min(POLL_CEILING);
                continue;
            }
            Some(other) => return Err(Error::Denied(format!("the login was refused: {other}"))),
            None => {}
        }
        if status >= 400 {
            return Err(Error::Denied(format!("the login failed: HTTP {status}")));
        }
        let grant = minted(&body)?;
        save(&parts, store, grant.clone())?;
        let chat = exchange(parts.spec, http, grant).await?;
        return keep(&parts, chat);
    }
    Err(Error::Denied("the login timed out".to_string()))
}
