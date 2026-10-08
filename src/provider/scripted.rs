use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::error::{Error, Result};

use super::{AssistantMessage, CompletionRequest, Message, Provider, StopReason, ToolCall};

pub(crate) enum Reply {
    Text(&'static str),
    Call(ToolCall),
    Fail(&'static str),
}

#[derive(Default)]
pub(crate) struct Scripted {
    replies: Arc<Mutex<VecDeque<Reply>>>,
    sent: Sent,
    prefixes: Prefixes,
}

pub(crate) type Sent = Arc<Mutex<Vec<Vec<Message>>>>;
pub(crate) type Prefixes = Arc<Mutex<Vec<String>>>;

impl Scripted {
    pub(crate) fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            replies: Arc::new(Mutex::new(replies.into_iter().collect())),
            sent: Sent::default(),
            prefixes: Prefixes::default(),
        }
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
        })
    }

    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        self.sent.lock().unwrap().push(request.messages.to_vec());
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
                Some(Reply::Fail(why)) => Err(Error::Config(why.to_string())),
                None => Err(Error::Config("the script has no reply left".to_string())),
            }
        })
    }
}
