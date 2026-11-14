use std::borrow::Cow;

use super::*;
use crate::endpoint::SessionValue;

impl Client {
    pub(in crate::provider) fn headers(&self, session: Option<&str>) -> Vec<(&str, Cow<'_, str>)> {
        let posted = session.is_some();
        let mut headers: Vec<_> = self
            .headers
            .iter()
            .filter(|(key, _)| {
                !(posted && key == "content-type"
                    || self
                        .session_headers
                        .get(key)
                        .is_some_and(|value| posted || *value == SessionValue::RequestId))
            })
            .map(|(key, value)| (key.as_str(), Cow::Borrowed(value.as_str())))
            .collect();
        for (key, value) in &self.session_headers {
            match (value, session) {
                (SessionValue::SessionId, Some(session)) => {
                    headers.push((key.as_str(), Cow::Owned(session.to_string())));
                }
                (SessionValue::SessionId, None) => {}
                (SessionValue::RequestId, _) => {
                    headers.push((key.as_str(), Cow::Owned(uuid::Uuid::now_v7().to_string())));
                }
            }
        }
        headers
    }

    pub(super) fn plan_responses(&self) -> bool {
        wire::spec(self.wire).subscription_session && self.auth.is_login()
    }
}
