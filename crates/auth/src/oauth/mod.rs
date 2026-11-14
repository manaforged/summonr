mod device;
mod pkce;
mod store;
mod token;

use std::collections::BTreeMap;
use std::sync::Arc;

use leyline::Session;
use serde::{Deserialize, Serialize};
use summonr_core::secret::Secret;

use crate::{Error, Issued, Parts, Prompt, Result, Store};

use store::{held, save, stored};
use token::{exchanged, refresh, settle};

pub(crate) use store::{parse_stored, stale};
#[cfg(test)]
pub(crate) use token::minted;
pub(crate) use token::stamped;

pub use summonr_types::oauth::{Grant, TokenStyle};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Spec {
    pub token_url: String,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<Secret>,
    #[serde(default)]
    pub refresh_token: Option<Secret>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub grant: Grant,
    #[serde(default)]
    pub device_url: Option<String>,
    #[serde(default)]
    pub exchange_url: Option<String>,
    #[serde(default)]
    pub authorize_url: Option<String>,
    #[serde(default)]
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub authorize_params: BTreeMap<String, String>,
    #[serde(default)]
    pub token_params: BTreeMap<String, String>,
    #[serde(default)]
    pub token_style: TokenStyle,
    #[serde(default)]
    pub pkce: bool,
    #[serde(default)]
    pub claim_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub required_scope: Option<String>,
}

impl Spec {
    pub(crate) fn exchange(&self) -> Option<&str> {
        self.exchange_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
    }

    pub fn check(&self) -> Result<()> {
        if self.grant == Grant::DeviceCode && self.device_url.is_none() {
            return Err(Error::Config(
                "a device_code login needs device_url".to_string(),
            ));
        }
        if self.grant == Grant::AuthorizationCode && self.authorize_url.is_none() {
            return Err(Error::Config(
                "an authorization_code login needs authorize_url".to_string(),
            ));
        }
        if self.grant == Grant::RefreshToken && self.refresh_token.is_none() {
            return Err(Error::Config(
                "a refresh_token login needs refresh_token".to_string(),
            ));
        }
        Ok(())
    }

    pub(super) fn refresh_seed(&self) -> Issued {
        Issued {
            access: String::new(),
            refresh: self
                .refresh_token
                .as_ref()
                .map(|token| token.expose().to_string()),
            expires: None,
            identity: None,
            client: None,
        }
    }

    pub(super) fn extend_token_form(&self, form: &mut Vec<(String, String)>, include_secret: bool) {
        if include_secret && let Some(secret) = &self.client_secret {
            form.push(("client_secret".to_string(), secret.expose().to_string()));
        }
        for (key, value) in &self.token_params {
            form.push((key.clone(), value.clone()));
        }
    }
}

impl TryFrom<&summonr_types::oauth::OAuth> for Spec {
    type Error = Error;

    fn try_from(row: &summonr_types::oauth::OAuth) -> Result<Self> {
        if row.token_url.trim().is_empty() || row.client_id.trim().is_empty() {
            return Err(Error::Config(
                "a login needs token_url and client_id".to_string(),
            ));
        }
        let spec = Self {
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
        spec.check()?;
        Ok(spec)
    }
}

pub(super) fn open_browser(prompt: &mut Prompt, uri: &str, code: &str, fallback: &str) {
    prompt.open(uri, code);
    if let Err(error) = crate::open::external(uri) {
        prompt.notice(&format!("the browser did not open ({error}); {fallback}"));
    }
}

pub(crate) async fn token(
    parts: Parts<'_>,
    http: &Session,
    shared: &Arc<dyn Store>,
) -> Result<String> {
    let store = shared.as_ref();
    if let Some(live) = held(&parts)? {
        return Ok(live.access);
    }
    let _turn = parts.renewing.lock().await;
    if let Some(live) = held(&parts)? {
        return Ok(live.access);
    }
    if let Some(grant) = stored(&parts, store)?
        && grant.fresh()
    {
        return exchanged(&parts, http, shared, grant, false).await;
    }
    let claim = claim(&parts, shared).await?;
    let outcome = renew(&parts, http, shared).await;
    drop(claim);
    outcome
}

async fn claim(parts: &Parts<'_>, store: &Arc<dyn Store>) -> Result<Box<dyn Send>> {
    let store = Arc::clone(store);
    let name = parts.name.to_string();
    tokio::task::spawn_blocking(move || store.claim(&name))
        .await
        .map_err(|error| {
            Error::Config(format!(
                "waiting for the {} login lock failed: {error}",
                parts.name
            ))
        })?
}

async fn renew(parts: &Parts<'_>, http: &Session, shared: &Arc<dyn Store>) -> Result<String> {
    let store = shared.as_ref();
    if let Some(grant) = stored(parts, store)? {
        if grant.fresh() {
            return exchanged(parts, http, shared, grant, true).await;
        }
        if grant.refresh.is_some() {
            return match refresh(parts.spec, http, &grant).await {
                Ok(next) => {
                    save(parts, store, &next)?;
                    exchanged(parts, http, shared, next, true).await
                }
                Err(error) if parts.spec.grant.interactive() => Err(Error::NeedsLogin {
                    name: parts.name.to_string(),
                    detail: format!("refresh failed ({error})"),
                }),
                Err(error) => Err(error),
            };
        }
    }
    if parts.spec.grant.interactive() {
        return Err(Error::NeedsLogin {
            name: parts.name.to_string(),
            detail: format!("run `summonr login {}`", parts.name),
        });
    }
    let minted = refresh(parts.spec, http, &parts.spec.refresh_seed()).await?;
    save(parts, store, &minted)?;
    exchanged(parts, http, shared, minted, true).await
}

pub(crate) async fn login(
    parts: Parts<'_>,
    http: &Session,
    shared: &Arc<dyn Store>,
    prompt: &mut Prompt,
) -> Result<()> {
    parts.spec.check()?;
    match parts.spec.grant {
        Grant::RefreshToken => {
            let minted = refresh(parts.spec, http, &parts.spec.refresh_seed()).await?;
            settle(&parts, http, shared, minted).await
        }
        Grant::DeviceCode => device::login(&parts, http, shared, prompt).await,
        Grant::AuthorizationCode => pkce::login(&parts, http, shared, prompt).await,
    }
}
