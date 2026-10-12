use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::error::{Error, Result};

const BASE: &str = "https://api.telegram.org/bot";
pub(crate) const MESSAGE_MAX: usize = 4096;
const MALFORMED: &str = "that does not look like a Telegram bot token (it should look like 123456789:AA...); copy it from @BotFather";
const TYPING: &str = "typing";
const UNAUTHORIZED: i64 = 401;
const NOT_FOUND: i64 = 404;
const CONFLICT: i64 = 409;
const TOO_MANY: u16 = 429;
const UPDATE_KINDS: &str = "[\"message\"]";
const TAKEN: &str = "another bridge or a webhook is already reading this bot's updates";
const REJECTED: &str = "Telegram rejected that token (HTTP 401). Copy it again from @BotFather.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    GetMe,
    GetUpdates,
    SendMessage,
    SendChatAction,
    SetMyCommands,
}

impl Method {
    fn name(self) -> &'static str {
        match self {
            Self::GetMe => "getMe",
            Self::GetUpdates => "getUpdates",
            Self::SendMessage => "sendMessage",
            Self::SendChatAction => "sendChatAction",
            Self::SetMyCommands => "setMyCommands",
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ChatKind {
    Private,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Chat {
    pub id: i64,
    #[serde(rename = "type", default)]
    pub kind: ChatKind,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct User {
    pub id: i64,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
}

impl User {
    pub(crate) fn describe(&self) -> String {
        let name: Vec<&str> = [&self.first_name, &self.last_name]
            .into_iter()
            .filter_map(Option::as_deref)
            .collect();
        let handle = self
            .username
            .as_deref()
            .map_or_else(|| "a user".to_string(), |user| format!("@{user}"));
        if name.is_empty() {
            format!("{handle} (id {})", self.id)
        } else {
            format!("{handle} ({}, id {})", name.join(" "), self.id)
        }
    }
}

pub(crate) struct Bot {
    http: leyline::Session,
    token: String,
}

impl Bot {
    pub(crate) fn new(token: &str) -> Result<Self> {
        let token = token.trim();
        if !well_formed(token) {
            return Err(Error::Config(MALFORMED.to_string()));
        }
        let limits = crate::limits::limits();
        let wait = Duration::from_secs(
            limits
                .gateway_poll_secs
                .saturating_add(limits.connect_secs.max(1)),
        );
        Ok(Self {
            http: crate::credentials::session(
                summonr_auth::builder(wait, limits.connect()),
                limits.retry_policy(crate::limits::Reach::Remote),
            )?,
            token: token.to_string(),
        })
    }

    fn failed(&self, name: &str, why: &str) -> Error {
        let why = crate::text::redact(why, [self.token.as_str()]);
        Error::Tool(format!("telegram {name}: {why}"))
    }

    fn url(&self, method: Method) -> String {
        format!("{BASE}{}/{}", self.token, method.name())
    }

    async fn post<T: DeserializeOwned>(&self, method: Method, body: &Value) -> Result<T> {
        let request = self.http.post(self.url(method)).json(body);
        let request = if method == Method::SendMessage {
            request.retry(send_policy())
        } else {
            request
        };
        self.call(method, request).await
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        request: leyline::RequestBuilder,
    ) -> Result<T> {
        let name = method.name();
        let response = request
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
        if let (true, Some(result)) = (reply.ok, reply.result) {
            return Ok(result);
        }
        match reply.error_code.unwrap_or_else(|| i64::from(status)) {
            UNAUTHORIZED => Err(Error::Setup(REJECTED.to_string())),
            NOT_FOUND if method == Method::GetMe => Err(Error::Config(MALFORMED.to_string())),
            CONFLICT => Err(Error::Config(TAKEN.to_string())),
            code => Err(Error::Tool(format!("telegram {name}: HTTP {code}"))),
        }
    }

    pub(crate) async fn me(&self) -> Result<Me> {
        self.post(Method::GetMe, &json!({})).await
    }

    pub(crate) async fn updates(&self, offset: i64) -> Result<Vec<Update>> {
        let wait = crate::limits::limits().gateway_poll_secs;
        let request = self.http.get(self.url(Method::GetUpdates)).query([
            ("offset", offset.to_string()),
            ("timeout", wait.to_string()),
            ("allowed_updates", UPDATE_KINDS.to_string()),
        ]);
        self.call(Method::GetUpdates, request).await
    }

    pub(crate) async fn send(&self, chat: i64, text: &str) -> Result<()> {
        for part in split(text, MESSAGE_MAX) {
            self.post::<Value>(
                Method::SendMessage,
                &json!({ "chat_id": chat, "text": part }),
            )
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn set_commands(&self, menu: &[(&str, &str)]) -> Result<()> {
        let commands: Vec<Value> = menu
            .iter()
            .map(|(command, description)| json!({ "command": command, "description": description }))
            .collect();
        self.post::<Value>(Method::SetMyCommands, &json!({ "commands": commands }))
            .await
            .map(drop)
    }

    pub(crate) async fn typing(&self, chat: i64) -> Result<()> {
        self.post::<Value>(
            Method::SendChatAction,
            &json!({ "chat_id": chat, "action": TYPING }),
        )
        .await
        .map(drop)
    }
}

fn well_formed(token: &str) -> bool {
    token.split_once(':').is_some_and(|(id, secret)| {
        !id.is_empty()
            && id.bytes().all(|byte| byte.is_ascii_digit())
            && !secret.is_empty()
            && secret.bytes().all(|byte| byte.is_ascii_graphic())
    })
}

fn send_policy() -> leyline::RetryPolicy {
    crate::limits::limits()
        .retry_policy(crate::limits::Reach::Remote)
        .retry_on([leyline::RetryTrigger::Status(TOO_MANY)])
        .allow_non_idempotent(true)
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
