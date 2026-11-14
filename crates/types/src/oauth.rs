use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use summonr_core::secret::Secret;

/// The OAuth grant a login uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    #[default]
    RefreshToken,
    DeviceCode,
    AuthorizationCode,
}

impl Grant {
    /// Whether the grant needs the person to act.
    #[must_use]
    pub fn interactive(self) -> bool {
        matches!(self, Self::DeviceCode | Self::AuthorizationCode)
    }

    /// Whether the grant opens a browser.
    #[must_use]
    pub fn browser(self) -> bool {
        matches!(self, Self::AuthorizationCode)
    }
}

/// The body format of a token request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenStyle {
    #[default]
    Form,
    Json,
}

/// How an issued token goes on a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenScheme {
    Bearer,
    ApiKey,
    GoogleKey,
}

/// A known value, or the unknown text kept for a warning.
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

/// The OAuth settings of a login endpoint.
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
    /// The grant, or the default when it is unset or unknown.
    #[must_use]
    pub fn grant(&self) -> Grant {
        Lenient::known(self.grant.as_ref()).unwrap_or_default()
    }

    /// The token request format, or the default when it is unset or unknown.
    #[must_use]
    pub fn token_style(&self) -> TokenStyle {
        Lenient::known(self.token_style.as_ref()).unwrap_or_default()
    }

    /// The token scheme, if it is set and known.
    #[must_use]
    pub fn scheme(&self) -> Option<TokenScheme> {
        Lenient::known(self.scheme.as_ref())
    }

    /// The keys whose values are unknown, with those values.
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
