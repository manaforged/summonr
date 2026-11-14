use std::path::Path;
use std::time::Duration;

use super::scope::{Origin, Scope, Sourced};
use crate::plugins::{Plugin, USER_CONFIG, Unset, Vars, expand};
use summonr_core::error::{Error, Result};
use summonr_core::secret::Secret;
use summonr_types::mcp::Transport;

pub(crate) use summonr_types::mcp::Entry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
        auth: Option<Box<crate::config::OAuth>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Helper {
    pub command: String,
    pub origin: Origin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Server {
    pub name: String,
    pub launch: Launch,
    pub key: String,
    pub allow: Option<std::collections::BTreeSet<String>>,
    pub idle: Option<Duration>,
    pub wait: Duration,
    pub helper: Option<Helper>,
}

enum Built {
    Ready(Box<Server>),
    Bare,
    Skipped(String),
}

fn server(mut entry: Entry, name: String, origin: Origin, workspace: &Path) -> Result<Built> {
    for (key, value) in [
        ("timeout_secs", entry.timeout_secs),
        ("idle_secs", entry.idle_secs),
    ] {
        if value == Some(0) {
            return Err(Error::Config(format!(
                "mcp {name} {key} must be at least 1"
            )));
        }
    }
    if let Some(reason) = refused(&entry, origin.scope) {
        return Ok(Built::Skipped(reason));
    }
    let root = dunce::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let key = format!(
        "{}-{}",
        super::catalog::stem(&name),
        summonr_core::text::fnv64(
            &serde_json::to_vec(&(root, &name, &entry.command, &entry.args, &entry.url))
                .unwrap_or_default(),
        )
    );
    let allow = entry.tools.clone().map(|tools| tools.into_iter().collect());
    let limits = crate::limits::limits();
    let idle = (entry.keep_alive != Some(true)).then(|| {
        entry
            .idle_secs
            .map_or(limits.mcp_idle(), Duration::from_secs)
    });
    let wait = entry
        .timeout_secs
        .map(Duration::from_secs)
        .or_else(|| {
            entry
                .timeout
                .map(Duration::from_millis)
                .filter(|wait| *wait >= limits.mcp_timeout_floor())
        })
        .unwrap_or_else(|| limits.mcp_call());
    let plugin = match origin.scope {
        Scope::Plugin => Plugin::open(origin.dir.clone()),
        Scope::Local | Scope::Project | Scope::User => None,
    };
    let paths = Vars {
        plugin: plugin.as_ref(),
        workspace: plugin.as_ref().map(|_| workspace),
        ..Vars::default()
    };
    let helper = entry.headers_helper.take().map(|command| Helper {
        command: expand(&command, &paths),
        origin,
    });
    let user_config = std::mem::take(&mut entry.user_config);
    let vars = Vars {
        user_config: Some(&user_config),
        env: Some(Unset::Kept),
        ..paths
    };
    let Some(launch) = launch(entry, &vars) else {
        return Ok(Built::Bare);
    };
    let helper = match launch {
        Launch::Http { .. } => helper,
        Launch::Stdio { .. } => None,
    };
    Ok(Built::Ready(Box::new(Server {
        launch,
        name,
        key,
        allow,
        idle,
        wait,
        helper,
    })))
}

fn refused(entry: &Entry, scope: Scope) -> Option<String> {
    match entry.transport {
        Some(Transport::Ws) => {
            return Some("type \"ws\" is not supported; use \"http\"".to_string());
        }
        Some(Transport::Sdk) => {
            return Some(
                "type \"sdk\" names a server only an SDK host application can register".to_string(),
            );
        }
        Some(Transport::Stdio | Transport::Http | Transport::Sse) | None => {}
    }
    match scope {
        Scope::Plugin
            if entry
                .headers_helper
                .as_deref()
                .is_some_and(|helper| helper.contains(USER_CONFIG)) =>
        {
            Some("headersHelper cannot use ${user_config.KEY}; put it in headers".to_string())
        }
        Scope::Plugin | Scope::Local | Scope::Project | Scope::User => None,
    }
}

fn launch(entry: Entry, vars: &Vars<'_>) -> Option<Launch> {
    let fill = |text: &str| expand(text, vars);
    let remote = match entry.transport {
        Some(Transport::Stdio) => false,
        Some(Transport::Http | Transport::Sse | Transport::Ws | Transport::Sdk) | None => true,
    };
    if remote && let Some(url) = entry.url {
        let mut headers: Vec<(String, String)> = entry
            .headers
            .iter()
            .map(|(key, value)| (key.clone(), fill(value)))
            .collect();
        if let Some(var) = entry.bearer_token_env_var
            && let Ok(token) = std::env::var(var)
        {
            headers.push(("Authorization".to_string(), format!("Bearer {token}")));
        }
        return Some(Launch::Http {
            url: fill(&url),
            headers,
            auth: entry.auth.map(|auth| {
                Box::new(crate::config::OAuth {
                    client_secret: auth
                        .client_secret
                        .as_ref()
                        .map(|value| Secret::new(fill(value.expose()))),
                    refresh_token: auth
                        .refresh_token
                        .as_ref()
                        .map(|value| Secret::new(fill(value.expose()))),
                    ..auth
                })
            }),
        });
    }
    let command = entry.command?;
    Some(Launch::Stdio {
        command: fill(&command),
        args: entry.args.iter().map(|arg| fill(arg)).collect(),
        env: entry
            .env
            .iter()
            .map(|(key, value)| (key.clone(), fill(value)))
            .collect(),
    })
}

pub(crate) struct Found {
    pub servers: Vec<Server>,
    pub waiting: Vec<String>,
    pub skipped: Vec<(String, String)>,
}

pub(crate) fn servers(workspace: &Path) -> Result<Found> {
    let super::scope::Resolved {
        entries,
        waiting,
        skipped,
    } = super::scope::resolve(workspace)?;
    let mut found = Found {
        servers: Vec::new(),
        waiting,
        skipped,
    };
    for (name, Sourced { entry, origin }) in entries {
        if entry.enabled == Some(false) {
            continue;
        }
        match server(entry, name.clone(), origin, workspace)? {
            Built::Ready(server) => found.servers.push(*server),
            Built::Bare => {}
            Built::Skipped(reason) => found.skipped.push((name, reason)),
        }
    }
    Ok(found)
}
