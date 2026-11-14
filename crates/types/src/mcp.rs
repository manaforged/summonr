use serde::Deserialize;
use std::collections::BTreeMap;

const DEFAULT_PORTS: [(&str, &str); 4] = [
    ("https", ":443"),
    ("wss", ":443"),
    ("http", ":80"),
    ("ws", ":80"),
];

/// The transport a Claude Code `type` field names.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Stdio,
    #[serde(alias = "streamable-http")]
    Http,
    Sse,
    Ws,
    Sdk,
}

/// One MCP server entry from a config file.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Entry {
    #[serde(rename = "type")]
    pub transport: Option<Transport>,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub url: Option<String>,
    #[serde(default, alias = "http_headers")]
    pub headers: BTreeMap<String, String>,
    #[serde(rename = "headersHelper", alias = "headers_helper")]
    pub headers_helper: Option<String>,
    pub bearer_token_env_var: Option<String>,
    pub enabled: Option<bool>,
    pub auth: Option<crate::OAuth>,
    pub tools: Option<Vec<String>>,
    pub idle_secs: Option<u64>,
    pub keep_alive: Option<bool>,
    pub timeout_secs: Option<u64>,
    pub timeout: Option<u64>,
    #[serde(default)]
    pub user_config: BTreeMap<String, String>,
}

impl Entry {
    /// Applies `top` over this entry; a `top` with no launch changes only its switches.
    pub fn overlay(&mut self, top: Self) {
        if top.command.is_none() && top.url.is_none() {
            self.enabled = top.enabled.or(self.enabled);
            self.tools = top.tools.or(self.tools.take());
            self.idle_secs = top.idle_secs.or(self.idle_secs);
            self.keep_alive = top.keep_alive.or(self.keep_alive);
            self.timeout_secs = top.timeout_secs.or(self.timeout_secs);
            self.timeout = top.timeout.or(self.timeout);
            self.headers_helper = top.headers_helper.or(self.headers_helper.take());
            self.env.extend(top.env);
            self.headers.extend(top.headers);
            self.user_config.extend(top.user_config);
            return;
        }
        let enabled = top.enabled.or(self.enabled);
        *self = top;
        self.enabled = enabled;
    }

    /// The URL or command line by which two entries count as one server.
    #[must_use]
    pub fn endpoint(&self) -> Option<String> {
        if let Some(url) = &self.url {
            return Some(same_url(url));
        }
        let command = self.command.as_ref()?;
        Some(
            std::iter::once(command)
                .chain(&self.args)
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

fn same_url(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let scheme = scheme.to_ascii_lowercase();
    let split = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(split);
    let (user, host) = authority.rsplit_once('@').unwrap_or(("", authority));
    let mut host = host.to_ascii_lowercase();
    let default = DEFAULT_PORTS
        .into_iter()
        .find(|(name, _)| *name == scheme)
        .map(|(_, port)| port);
    if let Some(bare) = default.and_then(|port| host.strip_suffix(port)) {
        host = bare.to_string();
    }
    let at = if user.is_empty() { "" } else { "@" };
    format!("{scheme}://{user}{at}{host}{}", path.trim_end_matches('/'))
}
