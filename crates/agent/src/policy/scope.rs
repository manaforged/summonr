use summonr_types::catalog::Builtin;

use super::{Policy, tool_matches};
use crate::mcp::SEPARATOR;
use summonr_types::mcp::CLAUDE_PREFIX;

/// The tools a named agent may use, from its `tools` allow list and `disallowedTools` deny list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scope {
    allow: Option<Vec<Listed>>,
    deny: Vec<Listed>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Listed {
    Builtin(Builtin),
    Mcp(String),
}

impl Listed {
    fn parse(raw: &str) -> Option<Self> {
        let name = raw.split_once('(').map_or(raw, |(head, _)| head).trim();
        if let Some(rest) = name.strip_prefix(CLAUDE_PREFIX) {
            return Some(Self::Mcp(rest.to_string()));
        }
        crate::hooks::builtin(name).map(Self::Builtin)
    }
}

impl Scope {
    /// The scope of an agent that lists `allow` (every tool when `None`) and denies `deny`.
    #[must_use]
    pub(crate) fn new(allow: Option<&[String]>, deny: Option<&[String]>) -> Self {
        let parsed = |list: &[String]| -> Vec<Listed> {
            list.iter().filter_map(|raw| Listed::parse(raw)).collect()
        };
        Self {
            allow: allow.map(parsed),
            deny: deny.map(parsed).unwrap_or_default(),
        }
    }

    /// Whether this scope lets the agent use built-in tool `kind`.
    #[must_use]
    pub(crate) fn builtin(&self, kind: Builtin) -> bool {
        let entry = Listed::Builtin(kind);
        !self.deny.contains(&entry)
            && self
                .allow
                .as_ref()
                .is_none_or(|allow| allow.contains(&entry))
    }

    /// Whether this scope lets the agent use the MCP tool `alias`, spelled `server__tool`.
    #[must_use]
    pub(crate) fn mcp(&self, alias: &str) -> bool {
        let denied = self.deny.iter().any(|entry| match entry {
            Listed::Mcp(pattern) => {
                tool_matches(alias, pattern) || alias.starts_with(&format!("{pattern}{SEPARATOR}"))
            }
            Listed::Builtin(_) => false,
        });
        !denied
            && self.allow.as_ref().is_none_or(|allow| {
                allow.iter().any(|entry| match entry {
                    Listed::Mcp(pattern) => tool_matches(alias, pattern),
                    Listed::Builtin(_) => false,
                })
            })
    }

    /// Whether some entry of the allow list names a tool the deny list leaves.
    #[must_use]
    pub(crate) fn resolves(&self) -> bool {
        self.allow.as_ref().is_none_or(|allow| {
            allow.iter().any(|entry| match entry {
                Listed::Builtin(kind) => self.builtin(*kind),
                Listed::Mcp(_) => true,
            })
        })
    }
}

impl Policy {
    /// Limits the tools this session may use to `scope`.
    pub(crate) fn set_scope(&mut self, scope: Scope) {
        self.scope = scope;
    }

    /// The agent tool scope this session runs under.
    #[must_use]
    pub(crate) fn scope(&self) -> &Scope {
        &self.scope
    }
}
