use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use serde::{Deserialize, Serialize};

use crate::durable::store;
use crate::error::{Error, Result};

const FILE: &str = "gateway.toml";
pub(crate) const CODE_LEN: usize = 6;
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub code: String,
    pub user: i64,
    pub chat: i64,
    pub expires: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Chat {
    pub chat: i64,
    pub session: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct State {
    #[serde(default)]
    pub users: Vec<i64>,
    #[serde(default)]
    pub pending: Vec<Pending>,
    #[serde(default)]
    pub chats: Vec<Chat>,
}

impl State {
    pub(crate) fn paired(&self, user: i64) -> bool {
        self.users.contains(&user)
    }

    pub(crate) fn waiting(&self, user: i64, now: i64) -> bool {
        self.pending
            .iter()
            .any(|pending| pending.user == user && pending.expires > now)
    }

    pub(crate) fn prune(&mut self, now: i64) {
        self.pending.retain(|pending| pending.expires > now);
    }

    pub(crate) fn offer(&mut self, user: i64, chat: i64, code: &str, now: i64) {
        self.prune(now);
        self.pending.retain(|pending| pending.user != user);
        let window = i64::try_from(crate::limits::limits().gateway_pair_secs).unwrap_or(i64::MAX);
        self.pending.push(Pending {
            code: code.to_string(),
            user,
            chat,
            expires: now.saturating_add(window),
        });
    }

    pub(crate) fn pair(&mut self, code: &str, now: i64) -> Option<Pending> {
        self.prune(now);
        let wanted = code.trim().to_ascii_uppercase();
        let index = self
            .pending
            .iter()
            .position(|pending| pending.code == wanted)?;
        let pending = self.pending.remove(index);
        if !self.paired(pending.user) {
            self.users.push(pending.user);
        }
        Some(pending)
    }

    pub(crate) fn unpair(&mut self, user: i64) -> bool {
        let before = self.users.len();
        self.users.retain(|known| *known != user);
        self.pending.retain(|pending| pending.user != user);
        self.users.len() != before
    }

    pub(crate) fn session(&self, chat: i64) -> Option<&str> {
        self.chats
            .iter()
            .find(|row| row.chat == chat)
            .map(|row| row.session.as_str())
    }

    pub(crate) fn set_session(&mut self, chat: i64, session: Option<String>) {
        self.chats.retain(|row| row.chat != chat);
        if let Some(session) = session {
            self.chats.push(Chat { chat, session });
        }
    }

    pub(crate) fn paired_chats(&self) -> Vec<i64> {
        let mut chats: Vec<i64> = self.chats.iter().map(|row| row.chat).collect();
        chats.extend(self.users.iter().copied());
        chats.sort_unstable();
        chats.dedup();
        chats
    }
}

pub(crate) fn code_from(bytes: &[u8]) -> Option<String> {
    let limit = ALPHABET.len() * (256 / ALPHABET.len());
    let code: String = bytes
        .iter()
        .filter(|byte| usize::from(**byte) < limit)
        .take(CODE_LEN)
        .map(|byte| char::from(ALPHABET[usize::from(*byte) % ALPHABET.len()]))
        .collect();
    (code.len() == CODE_LEN).then_some(code)
}

pub(crate) fn new_code() -> String {
    loop {
        let mut bytes = [0_u8; CODE_LEN * 4];
        OsRng.fill_bytes(&mut bytes);
        if let Some(code) = code_from(&bytes) {
            return code;
        }
    }
}

pub(crate) fn path() -> Result<PathBuf> {
    Ok(crate::prompt::summonr_home()?.join(FILE))
}

pub(crate) fn load() -> Result<State> {
    let path = path()?;
    let _guard = store::lock(&path, true)?;
    read(&path)
}

pub(crate) fn update<T>(change: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
    let path = path()?;
    let _guard = store::lock(&path, true)?;
    let mut state = read(&path)?;
    let out = change(&mut state)?;
    let text = toml::to_string(&state)
        .map_err(|error| Error::Config(format!("{}: {error}", path.display())))?;
    store::write(&path, text)?;
    Ok(out)
}

fn read(path: &Path) -> Result<State> {
    let text = store::read_or_empty(path)?;
    toml::from_str(&text).map_err(|error| Error::Config(format!("{}: {error}", path.display())))
}
