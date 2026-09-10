use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::OAuth;
use crate::error::{Error, ProviderFailureKind, Result};

const SKEW: Duration = Duration::from_secs(60);

fn deadline(seconds: u64) -> Instant {
    const MAX: u64 = 10 * 365 * 24 * 3600;
    Instant::now() + Duration::from_secs(seconds.min(MAX))
}
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

#[derive(Clone)]
pub(super) struct Auth {
    inner: Arc<Inner>,
}

enum Inner {
    Bearer(String),
    OAuth(Box<OAuthInner>),
}

struct OAuthInner {
    spec: OAuth,
    name: String,
    home: PathBuf,
    slot: Mutex<Option<Issued>>,
}

#[derive(Clone)]
struct Issued {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<Instant>,
}

#[derive(Deserialize)]
struct TokenBody {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct DeviceStart {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct Disk {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<u64>,
}

impl Auth {
    pub(super) fn bearer(key: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner::Bearer(key.into())),
        }
    }

    pub(super) fn oauth(spec: OAuth, name: impl Into<String>) -> Result<Self> {
        if spec.token_url.trim().is_empty() || spec.client_id.trim().is_empty() {
            return Err(Error::Config(
                "oauth auth needs token_url and client_id".to_string(),
            ));
        }
        if device(&spec) && spec.device_url.as_deref().is_none_or(str::is_empty) {
            return Err(Error::Config(
                "device_code grant needs device_url".to_string(),
            ));
        }
        Self::oauth_in(spec, name, crate::prompt::mage_home()?)
    }

    pub(super) fn oauth_in(spec: OAuth, name: impl Into<String>, home: PathBuf) -> Result<Self> {
        if spec.token_url.trim().is_empty() || spec.client_id.trim().is_empty() {
            return Err(Error::Config(
                "oauth auth needs token_url and client_id".to_string(),
            ));
        }
        if device(&spec) && spec.device_url.as_deref().is_none_or(str::is_empty) {
            return Err(Error::Config(
                "device_code grant needs device_url".to_string(),
            ));
        }
        Ok(Self {
            inner: Arc::new(Inner::OAuth(Box::new(OAuthInner {
                spec,
                name: name.into(),
                home,
                slot: Mutex::new(None),
            }))),
        })
    }

    pub(super) fn from_endpoint(endpoint: &crate::endpoint::Endpoint) -> Result<Self> {
        if let Some(spec) = endpoint.oauth.clone() {
            return Self::oauth(spec, &endpoint.name);
        }
        let key = endpoint
            .api_key
            .clone()
            .filter(|key| !key.trim().is_empty())
            .unwrap_or_else(|| "local".to_string());
        Ok(Self::bearer(key))
    }

    pub(super) async fn token(&self, http: &reqwest::Client) -> Result<String> {
        match &*self.inner {
            Inner::Bearer(key) => Ok(key.clone()),
            Inner::OAuth(inner) => {
                token(http, &inner.spec, &inner.name, &inner.home, &inner.slot).await
            }
        }
    }

    pub(super) async fn login(
        &self,
        http: &reqwest::Client,
        mut notice: impl FnMut(&str),
    ) -> Result<()> {
        match &*self.inner {
            Inner::Bearer(_) => Err(Error::Config(
                "this endpoint uses an API key, not device login".to_string(),
            )),
            Inner::OAuth(inner) => {
                if !device(&inner.spec) {
                    return Err(Error::Config(
                        "this endpoint is not device_code; set device_url".to_string(),
                    ));
                }
                let issued = device_login(http, &inner.spec, &mut notice).await?;
                save_disk(&inner.home, &inner.name, &issued)?;
                let chat = exchange(http, &inner.spec, issued).await?;
                store(&inner.slot, chat)?;
                Ok(())
            }
        }
    }

    pub(super) fn forget_live(&self) {
        if let Inner::OAuth(inner) = &*self.inner
            && let Ok(mut guard) = inner.slot.lock()
        {
            *guard = None;
        }
    }
}

fn device(spec: &OAuth) -> bool {
    spec.grant.as_deref() == Some("device_code")
        || spec
            .device_url
            .as_deref()
            .is_some_and(|url| !url.trim().is_empty())
}

async fn token(
    http: &reqwest::Client,
    spec: &OAuth,
    name: &str,
    home: &Path,
    slot: &Mutex<Option<Issued>>,
) -> Result<String> {
    if let Some(issued) = live(slot)? {
        return Ok(issued.access_token);
    }
    if let Some(issued) = load_disk(home, name)? {
        if issued.fresh() {
            return put_chat(http, spec, slot, issued).await;
        }
        if issued.refresh_token.is_some() {
            match refresh(http, spec, &issued).await {
                Ok(next) => {
                    save_disk(home, name, &next)?;
                    return put_chat(http, spec, slot, next).await;
                }
                Err(error) => {
                    if let Some(reloaded) = load_disk(home, name)?
                        && reloaded.fresh()
                        && reloaded.access_token != issued.access_token
                    {
                        return put_chat(http, spec, slot, reloaded).await;
                    }
                    if device(spec) {
                        return Err(Error::Config(format!(
                            "token refresh failed ({error}); run `summon login {name}`"
                        )));
                    }
                    return Err(error);
                }
            }
        }
    }
    if device(spec) {
        return Err(Error::Config(format!(
            "run `summon login {name}` for this endpoint"
        )));
    }
    let issued = client_or_refresh(http, spec).await?;
    save_disk(home, name, &issued)?;
    put_chat(http, spec, slot, issued).await
}

async fn put_chat(
    http: &reqwest::Client,
    spec: &OAuth,
    slot: &Mutex<Option<Issued>>,
    issued: Issued,
) -> Result<String> {
    let chat = exchange(http, spec, issued).await?;
    let access = chat.access_token.clone();
    store(slot, chat)?;
    Ok(access)
}

async fn exchange(http: &reqwest::Client, spec: &OAuth, issued: Issued) -> Result<Issued> {
    let Some(url) = spec
        .exchange_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    else {
        return Ok(issued);
    };
    let response = http
        .get(url)
        .header("Accept", "application/json")
        .bearer_auth(&issued.access_token)
        .send()
        .await
        .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::provider(
            ProviderFailureKind::Authentication,
            format!("token exchange HTTP {status}: {}", text.trim()),
        ));
    }
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
        Error::provider(
            ProviderFailureKind::Malformed,
            format!("token exchange: {error}"),
        )
    })?;
    let access = value
        .get("token")
        .or_else(|| value.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            Error::provider(
                ProviderFailureKind::Authentication,
                "token exchange has no token".to_string(),
            )
        })?;
    let expires_at = value
        .get("expires_at")
        .and_then(serde_json::Value::as_u64)
        .and_then(|unix| {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
            unix.checked_sub(now).map(deadline)
        })
        .or_else(|| {
            value
                .get("expires_in")
                .and_then(serde_json::Value::as_u64)
                .filter(|seconds| *seconds > 0)
                .map(deadline)
        });
    Ok(Issued {
        access_token: access.to_string(),
        refresh_token: issued.refresh_token,
        expires_at,
    })
}

fn live(slot: &Mutex<Option<Issued>>) -> Result<Option<Issued>> {
    let guard = slot
        .lock()
        .map_err(|_poison| Error::Config("oauth token lock poisoned".to_string()))?;
    Ok(guard.as_ref().filter(|issued| issued.fresh()).cloned())
}

fn store(slot: &Mutex<Option<Issued>>, issued: Issued) -> Result<()> {
    let mut guard = slot
        .lock()
        .map_err(|_poison| Error::Config("oauth token lock poisoned".to_string()))?;
    *guard = Some(issued);
    Ok(())
}

impl Issued {
    fn fresh(&self) -> bool {
        self.expires_at
            .is_none_or(|deadline| Instant::now() + SKEW < deadline)
    }
}

fn credentials_path(home: &Path, name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    let file = if safe == name {
        format!("{safe}.json")
    } else {
        format!("{safe}-{}.json", crate::hashline::line_hash(name))
    };
    home.join("credentials").join(file)
}

fn secret_name(name: &str) -> String {
    format!("oauth:{name}")
}

pub(crate) fn forget_legacy(name: &str) -> Result<bool> {
    let path = credentials_path(&crate::prompt::mage_home()?, name);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn load_disk(home: &Path, name: &str) -> Result<Option<Issued>> {
    let text = match crate::secret::get(&secret_name(name))? {
        Some((text, _)) => text,
        None => {
            let legacy = credentials_path(home, name);
            let Ok(text) = std::fs::read_to_string(&legacy) else {
                return Ok(None);
            };
            crate::secret::set(&secret_name(name), &text)?;
            std::fs::remove_file(&legacy)?;
            text
        }
    };
    let disk: Disk = serde_json::from_str(&text)
        .map_err(|error| Error::Config(format!("stored token for {name}: {error}")))?;
    if disk.access_token.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(from_disk(disk)))
}

fn save_disk(_home: &Path, name: &str, issued: &Issued) -> Result<()> {
    let disk = Disk {
        access_token: issued.access_token.clone(),
        refresh_token: issued.refresh_token.clone(),
        expires_at: issued.expires_at.map(unix_at),
    };
    let text = serde_json::to_string(&disk)?;
    crate::secret::set(&secret_name(name), &text)?;
    Ok(())
}

fn unix_at(deadline: Instant) -> u64 {
    let remain = deadline.saturating_duration_since(Instant::now());
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .saturating_add(remain)
        .as_secs()
}

fn from_disk(disk: Disk) -> Issued {
    let expires_at = disk.expires_at.map(|unix| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        deadline(unix.saturating_sub(now))
    });
    Issued {
        access_token: disk.access_token,
        refresh_token: disk.refresh_token,
        expires_at,
    }
}

async fn client_or_refresh(http: &reqwest::Client, spec: &OAuth) -> Result<Issued> {
    post_token(http, spec, grant(spec), spec.refresh_token.as_deref()).await
}

async fn refresh(http: &reqwest::Client, spec: &OAuth, issued: &Issued) -> Result<Issued> {
    post_token(http, spec, "refresh_token", issued.refresh_token.as_deref()).await
}

async fn post_token(
    http: &reqwest::Client,
    spec: &OAuth,
    grant_type: &str,
    refresh_token: Option<&str>,
) -> Result<Issued> {
    let mut form = vec![
        ("client_id", spec.client_id.as_str()),
        ("grant_type", grant_type),
    ];
    if let Some(secret) = spec.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    if let Some(refresh) = refresh_token {
        form.push(("refresh_token", refresh));
    }
    if let Some(scope) = spec.scope.as_deref() {
        form.push(("scope", scope));
    }
    let body = token_post(http, &spec.token_url, &form).await?;
    issued_from(body)
}

async fn token_post(http: &reqwest::Client, url: &str, form: &[(&str, &str)]) -> Result<TokenBody> {
    let response = http
        .post(url)
        .header("Accept", "application/json")
        .form(form)
        .send()
        .await
        .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let body: TokenBody = serde_json::from_str(&text).unwrap_or(TokenBody {
        access_token: None,
        refresh_token: None,
        expires_in: None,
        error: Some(format!("HTTP {status}")),
        error_description: Some(text.trim().to_string()),
    });
    if !status.is_success() && body.error.is_none() {
        return Err(Error::provider(
            ProviderFailureKind::Authentication,
            format!("oauth token HTTP {status}: {}", text.trim()),
        ));
    }
    Ok(body)
}

fn issued_from(body: TokenBody) -> Result<Issued> {
    if let Some(error) = body.error.filter(|error| !error.is_empty()) {
        let detail = body.error_description.unwrap_or_default();
        return Err(Error::provider(
            ProviderFailureKind::Authentication,
            format!("oauth {error} {detail}"),
        ));
    }
    let access = body
        .access_token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            Error::provider(
                ProviderFailureKind::Authentication,
                "oauth token response has no access_token",
            )
        })?;
    Ok(Issued {
        access_token: access,
        refresh_token: body.refresh_token.filter(|token| !token.trim().is_empty()),
        expires_at: body.expires_in.filter(|seconds| *seconds > 0).map(deadline),
    })
}

fn grant(spec: &OAuth) -> &'static str {
    match spec.grant.as_deref() {
        Some("refresh_token") => "refresh_token",
        Some("device_code") => DEVICE_GRANT,
        _ if spec.refresh_token.as_deref().is_some_and(|t| !t.is_empty()) => "refresh_token",
        _ => "client_credentials",
    }
}

async fn device_login(
    http: &reqwest::Client,
    spec: &OAuth,
    notice: &mut impl FnMut(&str),
) -> Result<Issued> {
    let url = spec
        .device_url
        .as_deref()
        .ok_or_else(|| Error::Config("device_code grant needs device_url".to_string()))?;
    let mut form = vec![("client_id", spec.client_id.as_str())];
    if let Some(scope) = spec.scope.as_deref() {
        form.push(("scope", scope));
    }
    let response = http
        .post(url)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await
        .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
    let text = response.text().await.unwrap_or_default();
    let start: DeviceStart = serde_json::from_str(&text).map_err(|error| {
        Error::provider(
            ProviderFailureKind::Malformed,
            format!("device code: {error}: {}", text.trim()),
        )
    })?;
    let uri = start
        .verification_uri_complete
        .as_deref()
        .filter(|uri| !uri.is_empty())
        .unwrap_or(&start.verification_uri);
    notice(&format!("open {uri}"));
    notice(&format!("code {}", start.user_code));
    let mut interval = Duration::from_secs(start.interval.filter(|n| *n > 0).unwrap_or(5));
    let deadline = deadline(start.expires_in.filter(|n| *n > 0).unwrap_or(900));
    loop {
        if Instant::now() >= deadline {
            return Err(Error::Config("device login expired".to_string()));
        }
        if !interval.is_zero() {
            tokio::time::sleep(interval).await;
        }
        let poll = [
            ("client_id", spec.client_id.as_str()),
            ("grant_type", DEVICE_GRANT),
            ("device_code", start.device_code.as_str()),
        ];
        let body = token_post(http, &spec.token_url, &poll).await?;
        match body.error.as_deref() {
            None => return issued_from(body),
            Some("authorization_pending") => {}
            Some("slow_down") => {
                interval += Duration::from_secs(5);
            }
            Some(other) => {
                let detail = body.error_description.unwrap_or_default();
                return Err(Error::provider(
                    ProviderFailureKind::Authentication,
                    format!("device login {other} {detail}"),
                ));
            }
        }
    }
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
