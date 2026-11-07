/// A command the desktop palette runs; the sidebar shows the ones a person pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NewAgent,
    NewBranch,
    NewTerminal,
    ClaudeCode,
    Split,
    ClosePane,
    Needing,
    Search,
    Model,
    Changes,
    Files,
    Worktrees,
    Jobs,
    Peers,
    Ledger,
    Steps,
    Settings,
    Models,
    Usage,
    Mcp,
    Plugins,
    Shortcuts,
}

const NAMES: [(Action, &str); 22] = [
    (Action::NewAgent, "new_agent"),
    (Action::NewBranch, "new_branch"),
    (Action::NewTerminal, "new_terminal"),
    (Action::ClaudeCode, "claude_code"),
    (Action::Split, "split"),
    (Action::ClosePane, "close_pane"),
    (Action::Needing, "needing"),
    (Action::Search, "search"),
    (Action::Model, "model"),
    (Action::Changes, "changes"),
    (Action::Files, "files"),
    (Action::Worktrees, "worktrees"),
    (Action::Jobs, "jobs"),
    (Action::Peers, "peers"),
    (Action::Ledger, "ledger"),
    (Action::Steps, "steps"),
    (Action::Settings, "settings"),
    (Action::Models, "models"),
    (Action::Usage, "usage"),
    (Action::Mcp, "mcp"),
    (Action::Plugins, "plugins"),
    (Action::Shortcuts, "shortcuts"),
];

impl Action {
    /// The sidebar rows before a person changes them.
    pub const PINNED: [Self; 3] = [Self::NewAgent, Self::NewTerminal, Self::Search];

    /// The name the config file stores.
    pub fn name(self) -> &'static str {
        NAMES
            .iter()
            .find(|(action, _)| *action == self)
            .map_or("", |(_, name)| name)
    }

    /// The action a stored name names, or None for a name this build does not know.
    pub fn parse(name: &str) -> Option<Self> {
        NAMES
            .iter()
            .find(|(_, stored)| *stored == name)
            .map(|(action, _)| *action)
    }
}
