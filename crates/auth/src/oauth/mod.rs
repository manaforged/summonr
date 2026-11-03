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
    let seed = Issued {
        access: String::new(),
        refresh: parts
            .spec
            .refresh_token
            .as_ref()
            .map(|token| token.expose().to_string()),
        expires: None,
        identity: None,
        client: None,
    };
    let minted = refresh(parts.spec, http, &seed).await?;
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
            let seed = Issued {
                access: String::new(),
                refresh: parts
                    .spec
                    .refresh_token
                    .as_ref()
                    .map(|token| token.expose().to_string()),
                expires: None,
                identity: None,
                client: None,
            };
            let minted = refresh(parts.spec, http, &seed).await?;
            settle(&parts, http, shared, minted).await
        }
        Grant::DeviceCode => device::login(&parts, http, shared, prompt).await,
        Grant::AuthorizationCode => pkce::login(&parts, http, shared, prompt).await,
    }
}
