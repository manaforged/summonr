use std::time::{Duration, Instant};

use leyline::Session;
use serde_json::Value;

use crate::{
    DEVICE_EXPIRY, Error, POLL_CEILING, POLL_FLOOR, POLL_STEP, Parts, Prompt, Result, Store,
    seconds,
};

use super::pkce::{challenge, random};
use super::token::{minted, post, settle, told};

const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

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
    let body = requested(parts, http, device_url, verifier.as_deref()).await?;
    let code = body
        .get("device_code")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Denied(format!("no device_code in the reply: {}", told(&body))))?;
    announce(&body, prompt);
    let wait = body
        .get("interval")
        .and_then(Value::as_u64)
        .map_or(POLL_STEP, seconds)
        .clamp(POLL_FLOOR, POLL_CEILING);
    let stop = Instant::now()
        + body
            .get("expires_in")
            .and_then(Value::as_u64)
            .map_or(DEVICE_EXPIRY, seconds);
    let poll = Poll {
        parts,
        http,
        code,
        verifier: verifier.as_deref(),
    };
    poll.until(store, wait, stop).await
}

async fn requested(
    parts: &Parts<'_>,
    http: &Session,
    device_url: &str,
    verifier: Option<&str>,
) -> Result<Value> {
    let mut form = vec![("client_id".to_string(), parts.spec.client_id.clone())];
    if let Some(scope) = &parts.spec.scope {
        form.push(("scope".to_string(), scope.clone()));
    }
    if let Some(verifier) = verifier {
        form.push(("code_challenge".to_string(), challenge(verifier)));
        form.push(("code_challenge_method".to_string(), "S256".to_string()));
    }
    let (status, body) = post(http, device_url, &form, parts.spec.token_style).await?;
    if status >= 400 {
        return Err(Error::Denied(format!(
            "the device request failed: HTTP {status} {}",
            told(&body)
        )));
    }
    Ok(body)
}

fn announce(body: &Value, prompt: &mut Prompt) {
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
}

struct Poll<'a> {
    parts: &'a Parts<'a>,
    http: &'a Session,
    code: &'a str,
    verifier: Option<&'a str>,
}

impl Poll<'_> {
    async fn until(&self, store: &dyn Store, mut wait: Duration, stop: Instant) -> Result<()> {
        while Instant::now() < stop {
            tokio::time::sleep(wait).await;
            let (status, body) = post(
                self.http,
                &self.parts.spec.token_url,
                &self.form(),
                self.parts.spec.token_style,
            )
            .await?;
            match body.get("error").and_then(Value::as_str) {
                Some("authorization_pending") => continue,
                Some("slow_down") => {
                    wait = (wait + POLL_STEP).min(POLL_CEILING);
                    continue;
                }
                Some(other) => {
                    return Err(Error::Denied(format!("the login was refused: {other}")));
                }
                None => {}
            }
            if status >= 400 {
                return Err(Error::Denied(format!("the login failed: HTTP {status}")));
            }
            let grant = minted(&body)?;
            return settle(self.parts, self.http, store, grant).await;
        }
        Err(Error::Denied("the login timed out".to_string()))
    }

    fn form(&self) -> Vec<(String, String)> {
        let spec = self.parts.spec;
        let mut form = vec![
            ("grant_type".to_string(), DEVICE_GRANT.to_string()),
            ("device_code".to_string(), self.code.to_string()),
            ("client_id".to_string(), spec.client_id.clone()),
        ];
        if let Some(verifier) = self.verifier {
            form.push(("code_verifier".to_string(), verifier.to_string()));
        }
        for (key, value) in &spec.token_params {
            form.push((key.clone(), value.clone()));
        }
        form
    }
}
