use std::time::{Duration, Instant};

use leyline::Session;
use serde_json::Value;

use crate::{Error, Parts, Prompt, Result, Store};

use super::pkce::{challenge, random};
use super::store::{keep, save};
use super::token::{exchange, minted, post};

const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const POLL_FLOOR: Duration = Duration::from_secs(1);
const POLL_CEILING: Duration = Duration::from_secs(30);

pub(super) async fn login(
    parts: &Parts<'_>,
    http: &Session,
    store: &dyn Store,
    prompt: &mut Prompt,
) -> Result<()> {
    let device_url = parts
        .spec
        .device_url
        .as_deref()
        .ok_or_else(|| Error::Config("a device_code login needs device_url".to_string()))?;
    let verifier = match parts.spec.pkce {
        true => Some(random(32)?),
        false => None,
    };
    let mut form = vec![("client_id".to_string(), parts.spec.client_id.clone())];
    if let Some(scope) = &parts.spec.scope {
        form.push(("scope".to_string(), scope.clone()));
    }
    if let Some(verifier) = &verifier {
        form.push(("code_challenge".to_string(), challenge(verifier)));
        form.push(("code_challenge_method".to_string(), "S256".to_string()));
    }
    let (status, body) = post(http, device_url, &form, parts.spec.token_style).await?;
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
    prompt.notice(&format!("open {uri} and enter {shown}"));
    if let Err(error) = crate::open::external(uri) {
        prompt.notice(&format!(
            "the browser did not open ({error}); open the link"
        ));
    }
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
        let mut form = vec![
            ("grant_type".to_string(), DEVICE_GRANT.to_string()),
            ("device_code".to_string(), code.to_string()),
            ("client_id".to_string(), parts.spec.client_id.clone()),
        ];
        if let Some(verifier) = &verifier {
            form.push(("code_verifier".to_string(), verifier.clone()));
        }
        for (key, value) in &parts.spec.token_params {
            form.push((key.clone(), value.clone()));
        }
        let (status, body) =
            post(http, &parts.spec.token_url, &form, parts.spec.token_style).await?;
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
        save(parts, store, grant.clone())?;
        let chat = exchange(parts.spec, http, grant).await?;
        return keep(parts, chat);
    }
    Err(Error::Denied("the login timed out".to_string()))
}
