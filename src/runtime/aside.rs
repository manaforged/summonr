use super::turn::{Missed, Reply};
use super::*;
use crate::workflow::Role;

impl<P: Provider> Loop<P> {
    pub(crate) async fn aside(
        &mut self,
        role: Role,
        system: &str,
        user: &str,
    ) -> Result<(String, Usage)> {
        let advertised = self.advertised_tools();
        let read_only = self.tools.is_read_only();
        self.tools.set_read_only(true);
        let result = self.aside_rounds(role, system, user, &advertised).await;
        self.tools.set_read_only(read_only);
        result
    }

    pub(super) async fn side_call(
        &mut self,
        role: Role,
        messages: &[Message],
        advertised: Option<&mail::Advertised>,
    ) -> Result<std::result::Result<Reply, Missed>> {
        let request = match advertised {
            Some(advertised) => self
                .request(role, messages, &advertised.tools, false)
                .with_tools_json(&advertised.json),
            None => self.request(role, messages, &[], false),
        };
        let options = CallOptions {
            stream: false,
            retry: RetryPolicy::None,
        };
        self.call(role, request, options).await
    }

    async fn aside_rounds(
        &mut self,
        role: Role,
        system: &str,
        user: &str,
        advertised: &mail::Advertised,
    ) -> Result<(String, Usage)> {
        let mut messages = vec![
            Message::system(system.to_string()),
            Message::user(user.to_string()),
        ];
        let mut usage = Usage::default();
        loop {
            if let Some(reason) = self.round_abort() {
                return Err(Error::Config(reason));
            }
            self.freeze_catalog();
            let assistant = match self.side_call(role, &messages, Some(advertised)).await? {
                Ok(reply) => reply.message,
                Err(missed) => return Err(missed.error.error()),
            };
            let text = assistant.content.clone().unwrap_or_default();
            usage.add(&assistant.usage);
            if let Some(reason) = self.round_abort() {
                return Err(Error::Config(reason));
            }
            let message = assistant.into_message();
            let calls = message.tool_calls.clone();
            messages.push(message);
            if calls.is_empty() {
                return Ok((text, usage));
            }
            for call in calls {
                let ran = match self.pre_tool(&call).await.blocked {
                    Some(reason) => Err(super::hooks::blocked(&reason)),
                    None => self.tools.execute_outcome(&call).await,
                };
                let (success, output) = match ran {
                    Ok(output) => (output.success, output.text),
                    Err(error) => (false, error.to_string()),
                };
                let text = self.tool_text(&call, success, output).await;
                messages.push(Message {
                    failed: !success,
                    ..Message::tool(call.id, text)
                });
            }
        }
    }
}
