use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::error::{Error, Result};

use super::{AssistantMessage, CompletionRequest, Message, Provider, StopReason, ToolCall};

pub(crate) enum Reply {
    Text(&'static str),
    Call(ToolCall),
    Calls(Vec<ToolCall>),
    Fail(&'static str),
}

type Hook = Arc<dyn Fn(usize) + Send + Sync>;

#[derive(Default)]
pub(crate) struct Scripted {
    replies: Arc<Mutex<VecDeque<Reply>>>,
    sent: Sent,
    prefixes: Prefixes,
    hook: Option<Hook>,
}

pub(crate) type Sent = Arc<Mutex<Vec<Vec<Message>>>>;
pub(crate) type Prefixes = Arc<Mutex<Vec<String>>>;

impl Scripted {
    pub(crate) fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            replies: Arc::new(Mutex::new(replies.into_iter().collect())),
            sent: Sent::default(),
            prefixes: Prefixes::default(),
            hook: None,
        }
    }

    pub(crate) fn on_complete(mut self, hook: impl Fn(usize) + Send + Sync + 'static) -> Self {
        self.hook = Some(Arc::new(hook));
        self
    }

    pub(crate) fn sent(&self) -> Sent {
        Arc::clone(&self.sent)
    }

    pub(crate) fn prefixes(&self) -> Prefixes {
        Arc::clone(&self.prefixes)
    }
}

impl Provider for Scripted {
    fn child(&self) -> Option<Self> {
        Some(Self {
            replies: Arc::clone(&self.replies),
            sent: Arc::clone(&self.sent),
            prefixes: Arc::clone(&self.prefixes),
            hook: self.hook.clone(),
        })
    }

    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        let call = {
            let mut sent = self.sent.lock().unwrap();
            sent.push(request.messages.to_vec());
            sent.len() - 1
        };
        if let Some(hook) = &self.hook {
            hook(call);
        }
        self.prefixes.lock().unwrap().push(request.prefix_hash());
        let reply = self.replies.lock().unwrap().pop_front();
        Box::pin(async move {
            match reply {
                Some(Reply::Text(text)) => Ok(AssistantMessage {
                    content: Some(text.to_string()),
                    ..AssistantMessage::default()
                }),
                Some(Reply::Call(call)) => Ok(AssistantMessage {
                    tool_calls: vec![call],
                    stop_reason: StopReason::ToolCalls,
                    ..AssistantMessage::default()
                }),
                Some(Reply::Calls(calls)) => Ok(AssistantMessage {
                    tool_calls: calls,
                    stop_reason: StopReason::ToolCalls,
                    ..AssistantMessage::default()
                }),
                Some(Reply::Fail(why)) => Err(Error::Config(why.to_string())),
                None => Err(Error::Config("the script has no reply left".to_string())),
            }
        })
    }
}
