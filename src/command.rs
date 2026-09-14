use Argument::{Bare, Text, Value};
use Category::{Advanced, Common, Contextual, Preferences, Shortcuts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    Common,
    Contextual,
    Preferences,
    Advanced,
    Shortcuts,
}

impl Category {
    pub(crate) const ALL: [Self; 5] = [Common, Contextual, Preferences, Advanced, Shortcuts];

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Common => "commands",
            Contextual => "plan actions",
            Preferences => "preferences · also in /settings",
            Advanced => "advanced commands",
            Shortcuts => "shortcuts",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlashCommand {
    Help,
    Settings,
    Auth,
    Login,
    Model,
    Workflow,
    Theme,
    Tools,
    Reasoning,
    Receipts,
    Session,
    Resume,
    Search,
    History,
    Branch,
    Tree,
    Retry,
    Stats,
    Effort,
    Compact,
    Name,
    Steps,
    Pane,
    Layout,
    Diff,
    Copy,
    Export,
    View,
    Agents,
    Modules,
    Bus,
    Network,
    Write,
    Approve,
    Fork,
    Clear,
    Plan,
    PlanAccept,
    PlanReject,
    Auto,
    Goal,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub command: SlashCommand,
    argument: Argument,
    pub(crate) category: Category,
}

impl CommandSpec {
    const fn category(mut self, category: Category) -> Self {
        self.category = category;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Argument {
    Bare,
    Value,
    Text,
}

const SPECS: &[CommandSpec] = &[
    spec("/help", "keys and commands", SlashCommand::Help, Bare),
    spec(
        "/model",
        "choose the active model; next or prev switches configured endpoints",
        SlashCommand::Model,
        Value,
    ),
    spec(
        "/diff",
        "changes against the base in a pane; /diff PATH shows one file's hunks",
        SlashCommand::Diff,
        Value,
    ),
    spec(
        "/view",
        "read a workspace file in a pane: /view PATH; o opens it in $EDITOR",
        SlashCommand::View,
        Value,
    ),
    spec(
        "/search",
        "find text in this conversation",
        SlashCommand::Search,
        Text,
    ),
    spec(
        "/history",
        "reuse a prompt you sent in this or another session",
        SlashCommand::History,
        Text,
    ),
    spec(
        "/resume",
        "choose a session, or /resume ID|NAME",
        SlashCommand::Resume,
        Value,
    ),
    spec("/settings", "open settings", SlashCommand::Settings, Bare),
    spec(
        "/login",
        "authenticate a provider",
        SlashCommand::Login,
        Value,
    ),
    spec("/auth", "alias for /login", SlashCommand::Auth, Value).category(Shortcuts),
    spec(
        "/workflow",
        "choose models for planning, implementation, and review",
        SlashCommand::Workflow,
        Text,
    )
    .category(Advanced),
    spec(
        "/theme",
        "change the interface theme",
        SlashCommand::Theme,
        Bare,
    )
    .category(Preferences),
    spec(
        "/tools",
        "compact or full tool output",
        SlashCommand::Tools,
        Bare,
    )
    .category(Preferences),
    spec(
        "/export",
        "write this session as markdown: /export [PATH]",
        SlashCommand::Export,
        Value,
    ),
    spec(
        "/reasoning",
        "reasoning: off · collapsed · expanded",
        SlashCommand::Reasoning,
        Value,
    )
    .category(Preferences),
    spec(
        "/receipts",
        "turn receipts: off · collapsed · expanded",
        SlashCommand::Receipts,
        Value,
    )
    .category(Preferences),
    spec(
        "/pane",
        "open a pane beside the chat: /pane files · ledger · diff · worktrees · off",
        SlashCommand::Pane,
        Value,
    ),
    spec(
        "/layout",
        "arrange the screen: focus · run · files · review · `chat | files 30%`",
        SlashCommand::Layout,
        Value,
    )
    .category(Advanced),
    spec(
        "/copy",
        "copy the last reply; /copy all copies the transcript",
        SlashCommand::Copy,
        Value,
    ),
    spec(
        "/session",
        "session id and log path",
        SlashCommand::Session,
        Bare,
    )
    .category(Advanced),
    spec(
        "/retry",
        "continue a failed or cancelled turn from its saved results (F5)",
        SlashCommand::Retry,
        Bare,
    ),
    spec(
        "/name",
        "name this session; bare shows the name",
        SlashCommand::Name,
        Value,
    ),
    spec(
        "/steps",
        "run a plan's `## Steps` one at a time: /steps PLAN.md · clear",
        SlashCommand::Steps,
        Value,
    )
    .category(Advanced),
    spec(
        "/stats",
        "token usage and cost per round",
        SlashCommand::Stats,
        Bare,
    ),
    spec(
        "/effort",
        "reasoning effort: none · low · medium · high, or bare to show",
        SlashCommand::Effort,
        Value,
    ),
    spec(
        "/agents",
        "show connected Summoner sessions",
        SlashCommand::Agents,
        Bare,
    )
    .category(Advanced),
    spec(
        "/modules",
        "list the built-in modules, or /modules NAME on|off",
        SlashCommand::Modules,
        Text,
    )
    .category(Advanced),
    spec(
        "/bus",
        "turn automatic replies to peers on or off",
        SlashCommand::Bus,
        Bare,
    )
    .category(Preferences),
    spec(
        "/net",
        "sandbox network: on, off, or once",
        SlashCommand::Network,
        Value,
    )
    .category(Advanced),
    spec(
        "/write",
        "approve a path outside the workspace for writes",
        SlashCommand::Write,
        Value,
    )
    .category(Advanced),
    spec(
        "/approve",
        "ask before each shell command: on or off",
        SlashCommand::Approve,
        Value,
    )
    .category(Advanced),
    spec(
        "/fork",
        "branch at the latest completed turn",
        SlashCommand::Fork,
        Bare,
    )
    .category(Shortcuts),
    spec(
        "/branch",
        "branch after an earlier turn",
        SlashCommand::Branch,
        Text,
    ),
    spec(
        "/clear",
        "start fresh context; keep workspace files",
        SlashCommand::Clear,
        Bare,
    ),
    spec(
        "/tree",
        "navigate related sessions",
        SlashCommand::Tree,
        Bare,
    )
    .category(Shortcuts),
    spec(
        "/compact",
        "summarize earlier turns, keep the latest",
        SlashCommand::Compact,
        Bare,
    ),
    spec("/plan", "enter plan mode", SlashCommand::Plan, Bare),
    spec(
        "/plan accept",
        "approve the saved plan; send a prompt to begin",
        SlashCommand::PlanAccept,
        Bare,
    )
    .category(Contextual),
    spec(
        "/plan reject",
        "leave plan mode without implementing",
        SlashCommand::PlanReject,
        Bare,
    )
    .category(Contextual),
    spec("/auto", "return to auto mode", SlashCommand::Auto, Bare).category(Contextual),
    spec(
        "/goal",
        "work toward an objective across turns",
        SlashCommand::Goal,
        Text,
    )
    .category(Advanced),
    spec("/exit", "quit summon", SlashCommand::Exit, Bare),
    spec("/quit", "alias for /exit", SlashCommand::Exit, Bare).category(Shortcuts),
];

const fn spec(
    name: &'static str,
    description: &'static str,
    command: SlashCommand,
    argument: Argument,
) -> CommandSpec {
    CommandSpec {
        name,
        description,
        command,
        argument,
        category: Common,
    }
}

#[must_use]
pub const fn command_specs() -> &'static [CommandSpec] {
    SPECS
}

#[must_use]
pub fn parse_command(line: &str) -> Option<SlashCommand> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if !tokens.first().is_some_and(|head| head.starts_with('/')) {
        return None;
    }
    (1..=tokens.len().min(2)).rev().find_map(|count| {
        let head = tokens[..count].join(" ");
        SPECS
            .iter()
            .find(|spec| spec.name == head)
            .map(|spec| spec.command)
    })
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;

pub(crate) fn argument(line: &str) -> Result<Option<&str>, String> {
    let Some(command) = parse_command(line) else {
        if let Some(head) = line
            .split_whitespace()
            .next()
            .filter(|head| head.starts_with('/'))
        {
            return Err(format!(
                "Unknown command `{head}`. Use /help or press Tab to complete."
            ));
        }
        return Ok(None);
    };
    let spec = SPECS
        .iter()
        .find(|spec| spec.command == command)
        .expect("registered command");
    let mut tail = line.trim();
    for _ in spec.name.split_whitespace() {
        tail = tail
            .split_once(char::is_whitespace)
            .map_or("", |(_, tail)| tail.trim());
    }
    if tail.is_empty() {
        return Ok(None);
    }
    match spec.argument {
        Bare => Err(format!("{} takes no arguments.", spec.name)),
        Text => Ok(Some(tail)),
        Value => {
            if let Some(quote) = tail
                .chars()
                .next()
                .filter(|quote| matches!(quote, '\'' | '"'))
            {
                tail = tail
                    .strip_prefix(quote)
                    .and_then(|tail| tail.strip_suffix(quote))
                    .ok_or_else(|| format!("Close the quoted argument for {}.", spec.name))?;
            }
            Ok((!tail.is_empty()).then_some(tail))
        }
    }
}
