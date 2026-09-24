use super::*;
use crate::catalog::{Cost, Model};
use crate::limits::{MODEL_PROBE, PROBE};

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
    [
        row.get("context_length"),
        row.get("context_window"),
        row.get("max_model_len"),
        row.get("max_context_length"),
        row.pointer("/top_provider/context_length"),
    ]
    .into_iter()
    .flatten()
    .find_map(tokens)
}

pub(crate) fn input_cap(text: &str) -> Option<u32> {
    let rest = text.split("maximum allowed length (").nth(1)?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok().filter(|value| *value > 0)
}

pub(super) fn tokens(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

pub(crate) fn response_failure(code: u16, text: &str, retry_after: Option<u64>) -> Error {
    let lower = text.to_lowercase();
    let kind = match code {
        401 | 403 => ProviderFailureKind::Authentication,
        429 => ProviderFailureKind::RateLimited,
        400 | 500..=599
            if lower.contains("tool call")
                && lower.contains("arguments")
                && lower.contains("json")
                && (lower.contains("parse")
                    || lower.contains("invalid")
                    || lower.contains("valid")) =>
        {
            ProviderFailureKind::MalformedToolCall
        }
        500..=599 => ProviderFailureKind::Server,
        400 if (lower.contains("context")
            && ["window", "length", "too long", "exceed"]
                .iter()
                .any(|word| lower.contains(word)))
            || lower.contains("maximum allowed length")
            || (lower.contains("input length") && lower.contains("exceed")) =>
        {
            ProviderFailureKind::ContextOverflow
        }
        _ => ProviderFailureKind::Http,
    };
    Error::Provider(ProviderFailure {
        kind,
        status: Some(code),
        message: format!("HTTP {code}: {}", reason(text)),
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
        .map_or_else(|| text.to_string(), str::to_string)
}

impl Client {
    pub(super) fn unreachable(&self, error: &dyn std::fmt::Display) -> Error {
        let host = crate::splash::host_of(Some(&self.base)).unwrap_or(&self.base);
        Error::provider(
            ProviderFailureKind::Transport,
            format!("cannot reach {host} — check the network or the base_url ({error})"),
        )
    }

    pub(super) async fn rejected(
        &self,
        mut response: leyline::Response,
        model: Option<&str>,
    ) -> Error {
        let code = response.status().as_u16();
        let after = response
            .header("retry-after")
            .and_then(|value| value.trim().parse().ok());
        let text = response.text().await.unwrap_or_default();
        self.explain(response_failure(code, text.trim(), after), model)
    }

    fn explain(&self, error: Error, model: Option<&str>) -> Error {
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
            (ProviderFailureKind::Authentication, _) => {
                format!("{name} rejected the key — run `summonr login {name}` to replace it")
            }
            (ProviderFailureKind::RateLimited, _) => format!(
                "{name} is rate limiting this key — wait{} and retry, or pick another model with /model",
                failure
                    .retry_after
                    .map_or_else(String::new, |seconds| format!(" {seconds}s"))
            ),
            (ProviderFailureKind::Http, Some(model))
                if failure.status == Some(404)
                    || (lower.contains("model")
                        && [
                            "not found",
                            "not exist",
                            "unknown",
                            "invalid",
                            "not supported",
                        ]
                        .iter()
                        .any(|word| lower.contains(word))) =>
            {
                let known: Vec<String> = crate::catalog::cached()
                    .remove(&self.name)
                    .map(|listing| listing.models.into_iter().map(|row| row.id).collect())
                    .unwrap_or_default();
                let hint = crate::endpoint::closest(model, &known)
                    .map_or_else(String::new, |near| format!("did you mean `{near}`? "));
                format!(
                    "{name} has no model `{model}` — {hint}`summonr endpoints` lists its models"
                )
            }
            (
                ProviderFailureKind::Server
                | ProviderFailureKind::Transport
                | ProviderFailureKind::ContextOverflow
                | ProviderFailureKind::MalformedToolCall
                | ProviderFailureKind::Malformed
                | ProviderFailureKind::Http,
                _,
            ) => return Error::Provider(failure),
        };
        failure.message = format!("{advice} ({})", failure.message);
        Error::Provider(failure)
    }

    pub async fn discover(endpoint: &crate::Endpoint) -> Result<Vec<Model>> {
        let client = Self::connect(endpoint)?;
        let covered = match crate::endpoint::auth_info(&endpoint.name) {
            Ok(info) if info.catalog_heading() == "Coding plans" => Cost::Plan,
            Ok(info) if info.keyless() => Cost::Free,
            Ok(_) | Err(_) => Cost::Unknown,
        };
        let mut models = tokio::time::timeout(MODEL_PROBE, client.catalog(covered))
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
            Some(Kind::RateLimited) => State::RateLimited,
            Some(Kind::Transport) => State::Offline,
            Some(
                Kind::Server
                | Kind::ContextOverflow
                | Kind::MalformedToolCall
                | Kind::Malformed
                | Kind::Http,
            )
            | None => State::Error,
        }
    }

    async fn catalog(&self, fallback: Cost) -> Result<Vec<Model>> {
        let base = &self.base;
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

    pub async fn context(&self, model: &str) -> Option<u32> {
        tokio::time::timeout(PROBE, self.probe(model))
            .await
            .ok()
            .flatten()
    }

    pub(super) async fn probe(&self, model: &str) -> Option<u32> {
        let base = &self.base;
        let field = match self.wire {
            Wire::Anthropic => Some("max_input_tokens"),
            Wire::Gemini => Some("inputTokenLimit"),
            Wire::Completions | Wire::Responses | Wire::Decisions => None,
        };
        if let Some(field) = field {
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
