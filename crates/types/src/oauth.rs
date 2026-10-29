use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use summonr_core::secret::Secret;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    #[default]
    RefreshToken,
    DeviceCode,
    AuthorizationCode,
}

impl Grant {
    #[must_use]
    pub fn interactive(self) -> bool {
        matches!(self, Self::DeviceCode | Self::AuthorizationCode)
    }

    #[must_use]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenScheme {
    Bearer,
    ApiKey,
    GoogleKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Lenient<T> {
    Known(T),
    Unknown(String),
}

impl<T: Copy> Lenient<T> {
    fn known(slot: Option<&Self>) -> Option<T> {
        match slot {
            Some(Self::Known(value)) => Some(*value),
            Some(Self::Unknown(_)) | None => None,
        }
    }

    fn unknown<'a>(key: &'static str, slot: Option<&'a Self>) -> Option<(&'static str, &'a str)> {
        match slot {
            Some(Self::Unknown(value)) => Some((key, value.as_str())),
            Some(Self::Known(_)) | None => None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct OAuth {
    pub token_url: String,
    pub client_id: String,
    #[serde(default)]
    pub label: Option<String>,
    pub client_secret: Option<Secret>,
    pub refresh_token: Option<Secret>,
    pub scope: Option<String>,
    #[serde(default)]
    pub grant: Option<Lenient<Grant>>,
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
    pub token_style: Option<Lenient<TokenStyle>>,
    #[serde(default)]
    pub scheme: Option<Lenient<TokenScheme>>,
    #[serde(default)]
    pub pkce: bool,
    #[serde(default)]
    pub claim_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub required_scope: Option<String>,
}

impl OAuth {
    #[must_use]
    pub fn grant(&self) -> Grant {
        Lenient::known(self.grant.as_ref()).unwrap_or_default()
    }

    #[must_use]
    pub fn token_style(&self) -> TokenStyle {
        Lenient::known(self.token_style.as_ref()).unwrap_or_default()
    }

    #[must_use]
    pub fn scheme(&self) -> Option<TokenScheme> {
        Lenient::known(self.scheme.as_ref())
    }

    #[must_use]
    pub fn unknown(&self) -> Vec<(&'static str, &str)> {
        [
            Lenient::unknown("grant", self.grant.as_ref()),
            Lenient::unknown("token_style", self.token_style.as_ref()),
            Lenient::unknown("scheme", self.scheme.as_ref()),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[cfg(test)]
#[path = "oauth_tests.rs"]
mod tests;
