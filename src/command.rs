#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlashCommand {
    Help,
    Settings,
    Auth,
    Login,
    Model,
    Workflow,
    Theme,
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
    View,
    Agents,
    Modules,
    Bus,
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
}

const SPECS: &[CommandSpec] = &[
    spec("/help", "keys and commands", SlashCommand::Help),
    spec("/settings", "open settings", SlashCommand::Settings),
    spec("/auth", "show provider authentication", SlashCommand::Auth),
    spec("/login", "authenticate a provider", SlashCommand::Login),
    spec("/model", "choose the active model", SlashCommand::Model),
    spec(
        "/workflow",
        "seat each step: the card, or NAME · off · set · clear · save · default",
        SlashCommand::Workflow,
    ),
    spec("/theme", "cycle the splash theme", SlashCommand::Theme),
    spec(
        "/pane",
        "open a pane beside the chat: /pane files · ledger · diff · worktrees · off",
        SlashCommand::Pane,
    ),
    spec(
        "/layout",
        "arrange the screen: focus · run · files · review · `chat | files 30%`",
        SlashCommand::Layout,
    ),
    spec(
        "/diff",
        "changes against the base in a pane; /diff PATH shows one file's hunks",
        SlashCommand::Diff,
    ),
    spec(
        "/copy",
        "copy the last reply; /copy all copies the transcript",
        SlashCommand::Copy,
    ),
    spec(
        "/view",
        "read a workspace file in a pane: /view PATH; o opens it in $EDITOR",
        SlashCommand::View,
    ),
    spec("/session", "session id and log path", SlashCommand::Session),
    spec(
        "/search",
        "find text in this conversation",
        SlashCommand::Search,
    ),
    spec(
        "/history",
        "find saved prompt text across sessions",
        SlashCommand::History,
    ),
    spec(
        "/resume",
        "choose a session, or /resume ID|NAME",
        SlashCommand::Resume,
    ),
    spec(
        "/retry",
        "continue a failed or cancelled turn from its saved results (F5)",
        SlashCommand::Retry,
    ),
    spec(
        "/name",
        "name this session; bare shows the name",
        SlashCommand::Name,
    ),
    spec(
        "/steps",
        "run a plan's `## Steps` one at a time: /steps PLAN.md · clear",
        SlashCommand::Steps,
    ),
    spec(
        "/stats",
        "token usage and cost per round",
        SlashCommand::Stats,
    ),
    spec(
        "/effort",
        "reasoning effort: none · low · medium · high, or bare to show",
        SlashCommand::Effort,
    ),
    spec(
        "/agents",
        "live summoner sessions on the bus",
        SlashCommand::Agents,
    ),
    spec(
        "/modules",
        "list the built-in modules, or /modules NAME on|off",
        SlashCommand::Modules,
    ),
    spec("/bus", "toggle bus auto-reply", SlashCommand::Bus),
    spec("/fork", "branch this session", SlashCommand::Fork),
    spec(
        "/branch",
        "branch after an earlier turn",
        SlashCommand::Branch,
    ),
    spec("/clear", "reset conversation history", SlashCommand::Clear),
    spec("/tree", "navigate related sessions", SlashCommand::Tree),
    spec(
        "/compact",
        "summarize earlier turns, keep the latest",
        SlashCommand::Compact,
    ),
    spec("/plan", "toggle plan mode", SlashCommand::Plan),
    spec(
        "/plan accept",
        "keep the journaled plan and return to auto",
        SlashCommand::PlanAccept,
    ),
    spec(
        "/plan reject",
        "leave plan mode without implementing",
        SlashCommand::PlanReject,
    ),
    spec("/auto", "enable full tools", SlashCommand::Auto),
    spec(
        "/goal",
        "work toward an objective across turns",
        SlashCommand::Goal,
    ),
    spec("/exit", "quit summon", SlashCommand::Exit),
    spec("/quit", "quit summon", SlashCommand::Exit),
];

const fn spec(name: &'static str, description: &'static str, command: SlashCommand) -> CommandSpec {
    CommandSpec {
        name,
        description,
        command,
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
