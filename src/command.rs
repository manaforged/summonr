//! Shared interactive command grammar.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlashCommand {
    Help,
    Settings,
    Model,
    Theme,
    Session,
    Stats,
    Agents,
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
    spec("/model", "choose the active model", SlashCommand::Model),
    spec("/theme", "cycle the splash theme", SlashCommand::Theme),
    spec("/session", "session id and log path", SlashCommand::Session),
    spec("/stats", "token usage per round", SlashCommand::Stats),
    spec(
        "/agents",
        "live summoner sessions on the bus",
        SlashCommand::Agents,
    ),
    spec("/bus", "toggle bus auto-reply", SlashCommand::Bus),
    spec("/fork", "branch this session", SlashCommand::Fork),
    spec("/clear", "reset conversation history", SlashCommand::Clear),
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
    let line = line.trim();
    if line == "/goal" || line.starts_with("/goal ") {
        return Some(SlashCommand::Goal);
    }
    if line == "/model" || line.starts_with("/model ") {
        return Some(SlashCommand::Model);
    }
    SPECS
        .iter()
        .find(|spec| spec.name == line)
        .map(|spec| spec.command)
}
