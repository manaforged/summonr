use super::*;
use crate::workflow::Role;

impl<P: Provider> Loop<P> {
    pub(crate) async fn aside(
        &mut self,
        role: Role,
        system: &str,
        user: &str,
    ) -> Result<(String, Usage)> {
        let tools = self.advertised_tools().to_vec();
        let plan = self.is_plan();
        self.tools.set_read_only(true);
        let result = self.aside_rounds(role, system, user, &tools).await;
        self.tools.set_read_only(plan);
        result
    }

    async fn aside_rounds(
        &mut self,
        role: Role,
        system: &str,
        user: &str,
        tools: &[ToolDefinition],
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
            let request = self.request(role, &messages, tools, false);
            self.freeze_catalog();
            let assistant = match self.complete_once(role, &request).await? {
                Ok(assistant) => assistant,
                Err(wait_error) => return Err(wait_error.error()),
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
                let (success, output) = match self.tools.execute_outcome(&call).await {
                    Ok(output) => (output.success, output.text),
                    Err(error) => (false, error.to_string()),
                };
                let text = self.tool_text(&call, success, output).await;
                messages.push(Message::tool(call.id, text));
            }
        }
    }
}
