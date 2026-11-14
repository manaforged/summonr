use summonr_types::catalog::Builtin;

use super::Policy;
use crate::mail::Tool as MailTool;
use crate::mcp::SEPARATOR;
use summonr_types::mcp::CLAUDE_PREFIX;

const CLAUDE_MAIL: &str = "SendMessage";

/// The tools a named agent may use, from its `tools` allow list and `disallowedTools` deny list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scope {
    allow: Option<Vec<Listed>>,
    deny: Vec<Listed>,
}

#[derive(Clone, Copy)]
enum Side {
    Allow,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Listed {
    Builtin(Builtin),
    Mail(Option<MailTool>),
    Mcp(Reach),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Reach {
    exact: Option<String>,
    prefix: Option<String>,
}

/// The stem of `pattern` when it ends in the `*` wildcard, or `None` when it names one tool.
fn stem(pattern: &str) -> Option<&str> {
    pattern.strip_suffix('*')
}

/// Whether tool `alias` matches `pattern`: a trailing `*` matches every alias with that stem, else only the alias itself.
pub(super) fn matches(alias: &str, pattern: &str) -> bool {
    stem(pattern).map_or(alias == pattern, |stem| alias.starts_with(stem))
}

impl Reach {
    fn of(pattern: &str, side: Side) -> Self {
        if let Some(stem) = stem(pattern) {
            return Self {
                exact: None,
                prefix: Some(stem.to_string()),
            };
        }
        let server = format!("{pattern}{SEPARATOR}");
        match side {
            Side::Deny => Self {
                exact: Some(pattern.to_string()),
                prefix: Some(server),
            },
            Side::Allow if pattern.contains(SEPARATOR) => Self {
                exact: Some(pattern.to_string()),
                prefix: None,
            },
            Side::Allow => Self {
                exact: None,
                prefix: Some(server),
            },
        }
    }

    fn alias(alias: &str) -> Self {
        Self {
            exact: Some(alias.to_string()),
            prefix: None,
        }
    }

    fn matches(&self, alias: &str) -> bool {
        self.exact.as_deref() == Some(alias)
            || self
                .prefix
                .as_deref()
                .is_some_and(|prefix| alias.starts_with(prefix))
    }

    fn covers(&self, other: &Self) -> bool {
        other
            .exact
            .as_deref()
            .is_none_or(|alias| self.matches(alias))
            && other.prefix.as_deref().is_none_or(|stem| {
                self.prefix
                    .as_deref()
                    .is_some_and(|prefix| stem.starts_with(prefix))
            })
    }
}

impl Listed {
    fn parse(raw: &str, side: Side) -> Option<Self> {
        let name = raw.split_once('(').map_or(raw, |(head, _)| head).trim();
        if let Some(rest) = name.strip_prefix(CLAUDE_PREFIX) {
            return Some(Self::Mcp(Reach::of(rest, side)));
        }
        if name == CLAUDE_MAIL {
            return Some(Self::Mail(None));
        }
        crate::hooks::builtin(name)
            .map(Self::Builtin)
            .or_else(|| MailTool::of(name).map(|tool| Self::Mail(Some(tool))))
    }

    fn covers(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Builtin(kind), Self::Builtin(other)) => kind == other,
            (Self::Mail(tool), Self::Mail(other)) => tool.is_none() || tool == other,
            (Self::Mcp(reach), Self::Mcp(other)) => reach.covers(other),
            (Self::Builtin(_) | Self::Mail(_) | Self::Mcp(_), _) => false,
        }
    }
}

impl Scope {
    /// The scope of an agent that lists `allow` (every tool when `None`) and denies `deny`.
    #[must_use]
    pub(crate) fn new(allow: Option<&[String]>, deny: Option<&[String]>) -> Self {
        let parsed = |list: &[String], side: Side| -> Vec<Listed> {
            list.iter()
                .filter_map(|raw| Listed::parse(raw, side))
                .collect()
        };
        Self {
            allow: allow.map(|list| parsed(list, Side::Allow)),
            deny: deny
                .map(|list| parsed(list, Side::Deny))
                .unwrap_or_default(),
        }
    }

    /// Whether this scope lets the agent use built-in tool `kind`.
    #[must_use]
    pub(crate) fn builtin(&self, kind: Builtin) -> bool {
        self.permits(&Listed::Builtin(kind))
    }

    /// Whether this scope lets the agent use the mail tool `tool`.
    #[must_use]
    pub(crate) fn mail(&self, tool: MailTool) -> bool {
        self.permits(&Listed::Mail(Some(tool)))
    }

    /// Whether this scope lets the agent use the MCP tool `alias`, spelled `server__tool`.
    #[must_use]
    pub(crate) fn mcp(&self, alias: &str) -> bool {
        self.permits(&Listed::Mcp(Reach::alias(alias)))
    }

    /// Whether some entry of the allow list names a tool the deny list leaves.
    #[must_use]
    pub(crate) fn resolves(&self) -> bool {
        self.allow
            .as_ref()
            .is_none_or(|allow| allow.iter().any(|entry| !self.denies(entry)))
    }

    /// The refusal for a call to `name`, a tool outside this scope.
    #[must_use]
    pub(crate) fn refusal(name: &str) -> String {
        format!("tool `{name}` is not one of this agent's tools")
    }

    fn denies(&self, entry: &Listed) -> bool {
        self.deny.iter().any(|denied| denied.covers(entry))
    }

    fn permits(&self, tool: &Listed) -> bool {
        !self.denies(tool)
            && self
                .allow
                .as_ref()
                .is_none_or(|allow| allow.iter().any(|entry| entry.covers(tool)))
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
