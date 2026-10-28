use super::failure::markers;
use super::*;
use crate::catalog::{Cost, Model, tokens};

pub(super) fn loaded(value: &Value) -> Option<u32> {
    value
        .pointer("/default_generation_settings/n_ctx")
        .or_else(|| value.get("n_ctx"))
        .and_then(tokens)
}

pub(super) fn listed(value: &Value, model: &str) -> Option<u32> {
    let rows = value.get("data")?.as_array()?;
    let row = rows
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(model))?;
    crate::catalog::window(row)
}

pub(crate) fn response_failure(code: u16, text: &str, retry_after: Option<u64>) -> Error {
    let lower = text.to_ascii_lowercase();
    let table = markers();
    let known = table.named(&lower);
    let detail = reason(text);
    Error::Provider(ProviderFailure {
        kind: known.map_or_else(|| table.kind(code, &lower), |(_, kind)| kind),
        status: Some(code),
        message: known.map_or_else(
            || format!("HTTP {code}: {detail}"),
            |(marker, _)| format!("HTTP {code}: {marker}: {detail}"),
        ),
        partial_output: false,
        retry_after: retry_after.or_else(|| retry_delay(text)),
    })
}

fn retry_delay(text: &str) -> Option<u64> {
    let body: Value = serde_json::from_str(text).ok()?;
    body.pointer("/error/details")?
        .as_array()?
        .iter()
        .find_map(|detail| {
            detail
                .get("retryDelay")?
                .as_str()?
                .strip_suffix('s')?
                .parse::<f64>()
                .ok()
        })
        .map(|seconds| seconds.ceil() as u64)
}

pub(super) fn reason(text: &str) -> String {
    let body: Value = serde_json::from_str(text).unwrap_or_default();
    let error = body.get("error").unwrap_or(&body);
    error
        .get("message")
        .or(Some(error))
        .and_then(Value::as_str)
        .map_or_else(
            || summonr_core::text::hide_key_echo(text),
            summonr_core::text::hide_key_echo,
        )
}

fn refused(error: &leyline::Error) -> bool {
    std::iter::successors(Some(error as &dyn std::error::Error), |cause| {
        cause.source()
    })
    .any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::ConnectionRefused)
    })
}

pub(crate) fn unreachable_message(
    name: &str,
    base: &str,
    refused: bool,
    error: &dyn std::fmt::Display,
) -> String {
    let host = crate::splash::host_of(Some(base)).unwrap_or(base);
    match crate::endpoint::start_command(base) {
        Some(start) => {
            let name = if name.is_empty() { host } else { name };
            format!("{name} is not running at {host}; start it with `{start}` ({error})")
        }
        None if refused && crate::endpoint::local(base) => format!(
            "nothing is listening at {host}; start the server or fix the base_url ({error})"
        ),
        None => format!("cannot reach {host}: check the network or the base_url ({error})"),
    }
}

impl Client {
    pub(super) fn unreachable(&self, error: &leyline::Error) -> Error {
        Error::provider(
            ProviderFailureKind::Transport,
            unreachable_message(&self.name, &self.base, refused(error), error),
        )
    }

    pub(super) async fn rejected(&self, response: leyline::Response, model: Option<&str>) -> Error {
        let code = response.status().as_u16();
        let after = response
            .header("retry-after")
            .and_then(|value| value.trim().parse().ok());
        let text = response.text().await.unwrap_or_default();
        let keyed = self.auth.is_login() || self.token().await.is_ok_and(|key| !key.is_empty());
        self.explain(response_failure(code, text.trim(), after), model, keyed)
    }

    fn explain(&self, error: Error, model: Option<&str>, keyed: bool) -> Error {
        let Error::Provider(mut failure) = error else {
            return error;
        };
        let name = if self.name.is_empty() {
            crate::splash::host_of(Some(&self.base)).unwrap_or(&self.base)
        } else {
            &self.name
        };
        let lower = failure.message.to_lowercase();
        let advice = match (failure.kind, model) {
            (ProviderFailureKind::UsageLimit, _) => {
                if lower.contains(markers().plan_cap.as_str()) {
                    format!(
                        "{name} reached this app's cap on your ChatGPT plan; open Manage usage at {} to raise it, or pick another model with /model",
                        markers().plan_settings
                    )
                } else {
                    format!(
                        "{name} exhausted its usage allowance; wait for its reset or select another account or provider"
                    )
                }
            }
            (ProviderFailureKind::Authentication, _) if !keyed => {
                failure.kind = ProviderFailureKind::MissingKey;
                format!("{name} needs a key and none is set; run `summonr login {name}` to add one")
            }
            (ProviderFailureKind::Authentication, _) => {
                format!("{name} rejected the key; run `summonr login {name}` to replace it")
            }
            (ProviderFailureKind::RateLimited, _) => format!(
                "{name} is rate limiting this key; wait{} and retry, or pick another model with /model",
                failure
                    .retry_after
                    .map_or_else(String::new, |seconds| format!(" {seconds}s"))
            ),
            (ProviderFailureKind::Http, Some(model))
                if failure.status == Some(404) || markers().missing_model(&lower) =>
            {
                let known: Vec<String> = crate::catalog::cached()
                    .get(&self.name)
                    .map(|listing| listing.models.iter().map(|row| row.id.clone()).collect())
                    .unwrap_or_default();
                let hint = crate::endpoint::closest(model, &known, crate::endpoint::Near::Relative)
                    .map_or_else(String::new, |near| format!("did you mean `{near}`? "));
                format!("{name} has no model `{model}`: {hint}`summonr endpoints` lists its models")
            }
            (
                ProviderFailureKind::MissingKey
                | ProviderFailureKind::Server
                | ProviderFailureKind::Transport
                | ProviderFailureKind::ContextOverflow
                | ProviderFailureKind::MalformedToolCall
                | ProviderFailureKind::Malformed
                | ProviderFailureKind::Empty
                | ProviderFailureKind::Http,
                _,
            ) => return Error::Provider(failure),
        };
        failure.message = format!("{advice} ({})", failure.message);
        Error::Provider(failure)
    }

    pub async fn discover(endpoint: &crate::Endpoint) -> Result<Vec<Model>> {
        if wire::spec(endpoint.wire).child() {
            let program = endpoint.base_url.as_deref().unwrap_or_default();
            if !std::path::Path::new(program).is_file()
                && crate::askpass::on_path(program).is_none()
            {
                return Err(Error::Config(format!(
                    "{program} not found; install Claude Code and run `claude auth login`"
                )));
            }
            super::super::agent::AgentClient::new(endpoint, program)
                .status()
                .await?;
            return Ok(crate::catalog::listing(&endpoint.name).map_or_else(
                || crate::catalog::offered(&endpoint.name, Cost::Plan),
                |listing| listing.models,
            ));
        }
        let client = Self::connect(endpoint)?;
        let covered = match crate::endpoint::auth_info(&endpoint.name) {
            _ if endpoint.oauth.is_some() => Cost::Plan,
            Ok(info) if info.plan() => Cost::Plan,
            Ok(info) if info.keyless() => Cost::Free,
            Ok(_) | Err(_) => Cost::Unknown,
        };
        let mut models = tokio::time::timeout(
            crate::limits::limits().model_probe(),
            client.catalog(covered),
        )
        .await
        .map_err(|elapsed| {
            Error::provider(
                ProviderFailureKind::Transport,
                format!("GET /models {elapsed}"),
            )
        })??;
        crate::catalog::enrich(&endpoint.name, endpoint.base_url.as_deref(), &mut models);
        crate::catalog::remember(&endpoint.name, models.clone())?;
        Ok(models)
    }

    #[must_use]
    pub fn state(error: &Error) -> crate::endpoint::State {
        use crate::endpoint::State;
        use ProviderFailureKind as Kind;
        match error.provider_failure().map(|failure| failure.kind) {
            Some(Kind::Authentication) => State::KeyRejected,
            Some(Kind::MissingKey) => State::NeedsKey,
            Some(Kind::RateLimited | Kind::UsageLimit) => State::RateLimited,
            Some(Kind::Transport) => State::Offline,
            Some(
                Kind::Server
                | Kind::ContextOverflow
                | Kind::MalformedToolCall
                | Kind::Malformed
                | Kind::Empty
                | Kind::Http,
            )
            | None => State::Error,
        }
    }

    async fn catalog(&self, fallback: Cost) -> Result<Vec<Model>> {
        let base = &self.models_base;
        let mut url = format!("{base}/models");
        let mut models = Vec::new();
        loop {
            let value = self.get(url.clone()).await?;
            models.extend(crate::catalog::parse(&value, fallback));
            let Some(token) = value.get("nextPageToken").and_then(Value::as_str) else {
                break;
            };
            let next = format!("{base}/models?pageToken={token}");
            if token.is_empty() || next == url {
                break;
            }
            url = next;
        }
        models.sort_by(|left, right| left.id.cmp(&right.id));
        models.dedup_by(|left, right| left.id == right.id);
        if models.is_empty() {
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                "GET /models returned no model ids",
            ));
        }
        Ok(models)
    }

    pub async fn context(&self, endpoint: &str, model: &str) -> Option<u32> {
        let local = crate::endpoint::local(&self.base);
        if !local && let Some(limit) = self.recorded(endpoint, model) {
            return Some(limit);
        }
        let probed = tokio::time::timeout(crate::limits::limits().probe(), self.probe(model))
            .await
            .ok()
            .flatten();
        if local {
            probed
        } else {
            probed.or_else(|| self.recorded(endpoint, model))
        }
    }

    fn recorded(&self, endpoint: &str, model: &str) -> Option<u32> {
        crate::catalog::context_limit(endpoint, model, Some(&self.base))
    }

    pub(super) async fn probe(&self, model: &str) -> Option<u32> {
        let base = &self.base;
        if let Some(field) = wire::spec(self.wire).probe {
            let mut url = url::Url::parse(&format!("{base}/models/")).ok()?;
            url.path_segments_mut()
                .ok()?
                .pop_if_empty()
                .push(model.strip_prefix("models/").unwrap_or(model));
            let value = self.get(url.to_string()).await.ok()?;
            return value.get(field).and_then(tokens);
        }
        let root = base.strip_suffix("/v1").unwrap_or(base);
        if let Ok(mut url) = url::Url::parse(&format!("{root}/props")) {
            url.query_pairs_mut().append_pair("model", model);
            if let Ok(value) = self.get(url.to_string()).await
                && let Some(limit) = loaded(&value)
            {
                return Some(limit);
            }
        }
        let value = self.get(format!("{base}/models")).await.ok()?;
        listed(&value, model)
    }
}
