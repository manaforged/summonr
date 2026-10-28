use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub url: Option<String>,
    #[serde(default, alias = "http_headers")]
    pub headers: BTreeMap<String, String>,
    pub bearer_token_env_var: Option<String>,
    pub enabled: Option<bool>,
    pub auth: Option<crate::OAuth>,
    pub tools: Option<Vec<String>>,
    pub idle_secs: Option<u64>,
    pub keep_alive: Option<bool>,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub user_config: BTreeMap<String, String>,
}

impl Entry {
    pub fn overlay(&mut self, top: Self) {
        if top.command.is_none() && top.url.is_none() {
            self.enabled = top.enabled.or(self.enabled);
            self.tools = top.tools.or(self.tools.take());
            self.idle_secs = top.idle_secs.or(self.idle_secs);
            self.keep_alive = top.keep_alive.or(self.keep_alive);
            self.timeout_secs = top.timeout_secs.or(self.timeout_secs);
            self.env.extend(top.env);
            self.headers.extend(top.headers);
            self.user_config.extend(top.user_config);
            return;
        }
        let enabled = top.enabled.or(self.enabled);
        *self = top;
        self.enabled = enabled;
    }
}
