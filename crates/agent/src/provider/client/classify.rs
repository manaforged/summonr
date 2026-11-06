use super::*;
use crate::provider::capture::{Sink, Stage};

impl Client {
    pub async fn classify(
        &self,
        model: &str,
        state: &Value,
        questions: &std::collections::BTreeMap<String, crate::classify::Question>,
    ) -> Result<crate::classify::Decision> {
        if !wire::spec(self.wire).classifies() {
            return Err(Error::Config(
                "this endpoint does not classify; see `summonr endpoint list`".to_string(),
            ));
        }
        let body = wire::decisions::body(model, state, questions);
        let session =
            std::env::var(crate::session::SESSION_ENV).unwrap_or_else(|_| "classify".to_string());
        let sink = Sink::open(self.wire, "classify", &session)?;
        if let Some(sink) = &sink {
            sink.write(Stage::Request, body.to_string().as_bytes())?;
        }
        let bytes = self
            .send(
                &self.base,
                serde_json::to_vec(&body)?,
                Some(model),
                "classify",
                None,
                false,
            )
            .await?
            .bytes()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
        if let Some(sink) = &sink {
            sink.write(Stage::Response, &bytes)?;
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|error| Error::provider(ProviderFailureKind::Malformed, error.to_string()))?;
        wire::decisions::decision(text)
    }
}
