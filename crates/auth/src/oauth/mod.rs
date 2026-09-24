mod device;
mod pkce;
mod store;
mod token;

use std::collections::BTreeMap;

use leyline::Session;
use serde::{Deserialize, Serialize};

use crate::{Error, Issued, Parts, Prompt, Result, Store};

use store::{held, save, stored};
use token::{exchanged, refresh};

pub(crate) use store::parse_stored;
pub(crate) use token::stamped;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    #[default]
    RefreshToken,
    DeviceCode,
    AuthorizationCode,
}

impl Grant {
    pub fn interactive(self) -> bool {
        matches!(self, Self::DeviceCode | Self::AuthorizationCode)
    }

    pub fn browser(self) -> bool {
        matches!(self, Self::AuthorizationCode)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenStyle {
    #[default]
    Form,
    Json,
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
}

impl Spec {
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

pub(crate) async fn token(parts: Parts<'_>, http: &Session, store: &dyn Store) -> Result<String> {
    if let Some(live) = held(&parts)? {
        return Ok(live.access);
    }
    if let Some(grant) = stored(&parts, store)?
        && grant.fresh()
    {
        return exchanged(&parts, http, grant).await;
    }
    let claim = store.claim(parts.name)?;
    let outcome = renew(&parts, http, store).await;
    drop(claim);
    outcome
}

async fn renew(parts: &Parts<'_>, http: &Session, store: &dyn Store) -> Result<String> {
    if let Some(grant) = stored(parts, store)? {
        if grant.fresh() {
            return exchanged(parts, http, grant).await;
        }
        if grant.refresh.is_some() {
            return match refresh(parts.spec, http, &grant).await {
                Ok(next) => {
                    save(parts, store, next.clone())?;
                    exchanged(parts, http, next).await
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
        refresh: parts.spec.refresh_token.clone(),
        expires: None,
        identity: None,
    };
    let minted = refresh(parts.spec, http, &seed).await?;
    save(parts, store, minted.clone())?;
    exchanged(parts, http, minted).await
}

pub(crate) async fn login(
    parts: Parts<'_>,
    http: &Session,
    store: &dyn Store,
    prompt: &mut Prompt,
) -> Result<()> {
    parts.spec.check()?;
    match parts.spec.grant {
        Grant::RefreshToken => {
            let seed = Issued {
                access: String::new(),
                refresh: parts.spec.refresh_token.clone(),
                expires: None,
                identity: None,
            };
            let minted = refresh(parts.spec, http, &seed).await?;
            save(&parts, store, minted)
        }
        Grant::DeviceCode => device::login(&parts, http, store, prompt).await,
        Grant::AuthorizationCode => pkce::login(&parts, http, store, prompt).await,
    }
}
