use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::error::{Error, Result};

const BASE: &str = "https://api.telegram.org/bot";
pub(crate) const MESSAGE_MAX: usize = 4096;
const TYPING: &str = "typing";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    GetMe,
    GetUpdates,
    SendMessage,
    SendChatAction,
}

impl Method {
    fn name(self) -> &'static str {
        match self {
            Self::GetMe => "getMe",
            Self::GetUpdates => "getUpdates",
            Self::SendMessage => "sendMessage",
            Self::SendChatAction => "sendChatAction",
        }
    }
}

#[derive(Deserialize)]
struct Reply<T> {
    ok: bool,
    result: Option<T>,
    error_code: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct Me {
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Message {
    pub chat: Chat,
    #[serde(default)]
    pub from: Option<User>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Chat {
    pub id: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct User {
    pub id: i64,
}

pub(crate) struct Bot {
    http: leyline::Session,
    token: String,
}

impl Bot {
    pub(crate) fn new(token: &str) -> Result<Self> {
        let limits = crate::limits::limits();
        let wait = Duration::from_secs(
            limits
                .gateway_poll_secs
                .saturating_add(limits.connect_secs.max(1)),
        );
        Ok(Self {
            http: crate::credentials::session(
                summonr_auth::builder(wait, limits.connect()),
                limits.retry_policy(),
            )?,
            token: token.trim().to_string(),
        })
    }

    fn failed(&self, name: &str, why: &str) -> Error {
        let why = crate::text::redact(why, [self.token.as_str()]);
        Error::Tool(format!("telegram {name}: {why}"))
    }

    async fn call<T: DeserializeOwned>(&self, method: Method, body: &Value) -> Result<T> {
        let name = method.name();
        let url = format!("{BASE}{}/{name}", self.token);
        let response = self
            .http
            .post(url)
            .json(body)
            .send()
            .await
            .map_err(|error| self.failed(name, &error.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .await
            .map_err(|error| self.failed(name, &format!("HTTP {status}: {error}")))?;
        let reply: Reply<T> = serde_json::from_str(&text)
            .map_err(|error| self.failed(name, &format!("HTTP {status}: {error}")))?;
        match (reply.ok, reply.result) {
            (true, Some(result)) => Ok(result),
            (true | false, Some(_) | None) => Err(Error::Tool(format!(
                "telegram {name}: HTTP {}",
                reply.error_code.unwrap_or_else(|| i64::from(status))
            ))),
        }
    }

    pub(crate) async fn me(&self) -> Result<Me> {
        self.call(Method::GetMe, &json!({})).await
    }

    pub(crate) async fn updates(&self, offset: i64) -> Result<Vec<Update>> {
        let wait = crate::limits::limits().gateway_poll_secs;
        self.call(
            Method::GetUpdates,
            &json!({ "offset": offset, "timeout": wait, "allowed_updates": ["message"] }),
        )
        .await
    }

    pub(crate) async fn send(&self, chat: i64, text: &str) -> Result<()> {
        for part in split(text, MESSAGE_MAX) {
            self.call::<Value>(
                Method::SendMessage,
                &json!({ "chat_id": chat, "text": part }),
            )
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn typing(&self, chat: i64) -> Result<()> {
        self.call::<Value>(
            Method::SendChatAction,
            &json!({ "chat_id": chat, "action": TYPING }),
        )
        .await
        .map(drop)
    }
}

pub(crate) fn split(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut size = 0;
    for line in text.split_inclusive('\n') {
        let length = line.chars().count();
        if size + length > max && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
            size = 0;
        }
        if length <= max {
            current.push_str(line);
            size += length;
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        for chunk in chars.chunks(max) {
            parts.push(chunk.iter().collect());
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
        .into_iter()
        .map(|part| part.trim_end_matches('\n').to_string())
        .filter(|part| !part.trim().is_empty())
        .collect()
}
