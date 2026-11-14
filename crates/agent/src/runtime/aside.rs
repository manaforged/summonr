use super::hooks::{Screen, blocked};
use super::turn::{Missed, Reply};
use super::*;
use crate::workflow::Role;

struct ReadOnly<'a, P> {
    owner: &'a mut Loop<P>,
    read_only: bool,
}

impl<P> Drop for ReadOnly<'_, P> {
    fn drop(&mut self) {
        self.owner.tools.set_read_only(self.read_only);
    }
}

impl<P: Provider> Loop<P> {
    pub(crate) async fn aside(
        &mut self,
        role: Role,
        system: &str,
        user: &str,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(String, Usage)> {
        let advertised = self.advertised_tools();
        let read_only = self.tools.is_read_only();
        self.tools.set_read_only(true);
        let held = ReadOnly {
            owner: self,
            read_only,
        };
        held.owner
            .aside_rounds(role, system, user, &advertised, events, emit)
            .await
    }

    pub(super) async fn side_call(
        &mut self,
        role: Role,
        messages: &[Message],
        advertised: Option<&mail::Advertised>,
    ) -> Result<std::result::Result<Reply, Missed>> {
        let options = CallOptions {
            stream: false,
            retry: RetryPolicy::None,
        };
        self.call(
            role,
            super::turn::CallSource::Slice {
                messages,
                advertised,
            },
            options,
        )
        .await
    }

    async fn aside_rounds(
        &mut self,
        role: Role,
        system: &str,
        user: &str,
        advertised: &mail::Advertised,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
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
            for mut call in calls {
                let (ran, context) = match self.gate(&mut call, events, emit).await? {
                    Screen::Blocked(reason) => (Err(blocked(&reason)), None),
                    Screen::Run(context) => (self.tools.execute_outcome(&call).await, context),
                };
                let (success, output) = match ran {
                    Ok(output) => (output.success, output.text),
                    Err(error) => (false, error.to_string()),
                };
                let text = self.tool_text(&call, success, output, context).await;
                messages.push(Message {
                    failed: !success,
                    ..Message::tool(call.id, text)
                });
            }
        }
    }
}
