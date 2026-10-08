use super::*;

impl Client {
    pub async fn classify(
        &self,
        model: &str,
        state: &Value,
        questions: &std::collections::BTreeMap<String, crate::classify::Question>,
    ) -> Result<crate::classify::Decision> {
        if self.wire != Wire::Decisions {
            return Err(Error::Config(
                "this endpoint does not classify; see `summonr endpoints`".to_string(),
            ));
        }
        let body = wire::decisions::body(model, state, questions);
        let sink = super::capture::Sink::open(self.wire, "classify");
        if let Some(sink) = &sink {
            sink.write("request", &body.to_string());
        }
        let text = self
            .send(
                &self.base,
                serde_json::to_vec(&body)?,
                Some(model),
                "classify",
                None,
                false,
            )
            .await?
            .text()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
        if let Some(sink) = &sink {
            sink.write("response", &text);
        }
        wire::decisions::decision(&text)
    }
}
