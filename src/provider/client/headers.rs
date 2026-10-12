use super::*;

impl Client {
    pub(in crate::provider) fn headers(
        &self,
        session: Option<&str>,
    ) -> Vec<(&str, std::borrow::Cow<'_, str>)> {
        let mut headers: Vec<_> = self
            .headers
            .iter()
            .filter(|(key, _)| {
                let posted = session.is_some();
                !(posted && key == "content-type"
                    || self.opencode
                        && (key == "x-opencode-request" || posted && key == "x-opencode-session"))
            })
            .map(|(key, value)| (key.as_str(), std::borrow::Cow::Borrowed(value.as_str())))
            .collect();
        if self.opencode {
            if let Some(session) = session {
                headers.push((
                    "x-opencode-session",
                    std::borrow::Cow::Owned(session.to_string()),
                ));
            }
            headers.push((
                "x-opencode-request",
                std::borrow::Cow::Owned(uuid::Uuid::now_v7().to_string()),
            ));
        }
        if let Some(session) = session.filter(|_| self.plan_responses()) {
            headers.push(("session-id", std::borrow::Cow::Owned(session.to_string())));
        }
        headers
    }

    pub(super) fn plan_responses(&self) -> bool {
        self.wire == Wire::Responses && self.auth.is_login()
    }
}
