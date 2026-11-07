/// A command the desktop palette runs; the sidebar shows the ones a person pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NewAgent,
    NewTerminal,
    NewWorkflow,
    ClaudeCode,
    RunSetup,
    Run,
    OpenPr,
    Merge,
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
    Workflows,
    Shortcuts,
}

const NAMES: [(Action, &str); 27] = [
    (Action::NewAgent, "new_agent"),
    (Action::NewTerminal, "new_terminal"),
    (Action::NewWorkflow, "new_workflow"),
    (Action::ClaudeCode, "claude_code"),
    (Action::RunSetup, "run_setup"),
    (Action::Run, "run"),
    (Action::OpenPr, "open_pr"),
    (Action::Merge, "merge"),
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
    (Action::Workflows, "workflows"),
    (Action::Shortcuts, "shortcuts"),
];

impl Action {
    /// The sidebar rows before a person changes them.
    pub const PINNED: [Self; 4] = [
        Self::NewAgent,
        Self::NewTerminal,
        Self::NewWorkflow,
        Self::Search,
    ];

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
