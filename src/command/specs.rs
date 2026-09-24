use super::Argument::{self, Bare, Text, Value};
use super::Category::{self, Developer, Files, Model, Session, Settings};
use super::{CommandSpec, SlashCommand};

pub(super) const SPECS: &[CommandSpec] = &[
    spec(
        "/resume",
        "open another session",
        SlashCommand::Resume,
        Value,
        Session,
    )
    .usage("[ID|NAME]"),
    spec(
        "/name",
        "name this session",
        SlashCommand::Name,
        Value,
        Session,
    )
    .usage("[TEXT]"),
    spec(
        "/clear",
        "start a fresh context; files stay",
        SlashCommand::Clear,
        Bare,
        Session,
    ),
    spec(
        "/compact",
        "summarize earlier turns",
        SlashCommand::Compact,
        Bare,
        Session,
    ),
    spec(
        "/retry",
        "continue a failed or cancelled turn (F5)",
        SlashCommand::Retry,
        Bare,
        Session,
    ),
    spec(
        "/undo",
        "undo the last turn; files N restores files",
        SlashCommand::Undo,
        Value,
        Session,
    )
    .usage("[files [N]]"),
    spec(
        "/fork",
        "fork this session; TEXT picks a turn",
        SlashCommand::Fork,
        Text,
        Session,
    )
    .usage("[TEXT]"),
    spec(
        "/export",
        "write this session as markdown",
        SlashCommand::Export,
        Value,
        Session,
    )
    .usage("[PATH]"),
    spec(
        "/copy",
        "copy the last reply; all copies everything",
        SlashCommand::Copy,
        Value,
        Session,
    )
    .usage("[all]"),
    spec(
        "/search",
        "find text in this conversation",
        SlashCommand::Search,
        Text,
        Session,
    )
    .usage("TEXT"),
    spec(
        "/history",
        "reuse an earlier prompt",
        SlashCommand::History,
        Text,
        Session,
    )
    .usage("[TEXT]"),
    spec(
        "/status",
        "session id, log path, and model",
        SlashCommand::Status,
        Bare,
        Session,
    ),
    spec(
        "/usage",
        "token spend; session profiles this session",
        SlashCommand::Usage,
        Value,
        Session,
    )
    .usage("[session]"),
    spec(
        "/agents",
        "list sessions on the peer bus",
        SlashCommand::Agents,
        Bare,
        Session,
    ),
    spec("/exit", "quit summonr", SlashCommand::Exit, Bare, Session),
    spec(
        "/model",
        "choose the model; LEVEL sets the effort",
        SlashCommand::Model,
        Value,
        Model,
    )
    .usage("[NAME] [LEVEL]"),
    spec(
        "/workflow",
        "choose models for plan, work, and review",
        SlashCommand::Workflow,
        Text,
        Model,
    )
    .usage("[NAME]"),
    spec(
        "/login",
        "sign in to a provider or store its key",
        SlashCommand::Login,
        Value,
        Model,
    )
    .usage("[NAME]"),
    spec(
        "/plan",
        "enter plan mode: read and plan, no edits",
        SlashCommand::Plan,
        Bare,
        Model,
    ),
    spec(
        "/plan accept",
        "run the saved plan",
        SlashCommand::PlanAccept,
        Bare,
        Model,
    ),
    spec(
        "/plan reject",
        "leave plan mode without running the plan",
        SlashCommand::PlanReject,
        Bare,
        Model,
    ),
    spec(
        "/auto",
        "return to auto mode",
        SlashCommand::Auto,
        Bare,
        Model,
    ),
    spec(
        "/goal",
        "work toward an objective across turns",
        SlashCommand::Goal,
        Text,
        Model,
    )
    .usage("TEXT"),
    spec(
        "/steps",
        "run a plan's `## Steps` one at a time",
        SlashCommand::Steps,
        Value,
        Model,
    )
    .usage("PLAN.md|clear"),
    spec(
        "/diff",
        "changes against the base, or one PATH",
        SlashCommand::Diff,
        Value,
        Files,
    )
    .usage("[PATH]"),
    spec(
        "/view",
        "read a file in a pane",
        SlashCommand::View,
        Value,
        Files,
    )
    .usage("PATH"),
    spec(
        "/pane",
        "files, ledger, diff, worktrees, or a layout",
        SlashCommand::Pane,
        Value,
        Files,
    )
    .usage("[NAME]"),
    spec(
        "/settings",
        "theme, tool output, reasoning, modules, keys",
        SlashCommand::Settings,
        Bare,
        Settings,
    ),
    spec(
        "/permissions",
        "sandbox network, write paths, approval",
        SlashCommand::Permissions,
        Text,
        Settings,
    )
    .usage("[net|write|approve]"),
    spec(
        "/help",
        "list commands and keys",
        SlashCommand::Help,
        Bare,
        Settings,
    ),
    spec(
        "/demo",
        "show chat rendering: markdown, mermaid, charts, images, diffs",
        SlashCommand::Demo,
        Bare,
        Developer,
    ),
];

pub(super) const ALIASES: &[(&str, &str)] = &[
    ("/auth", "/login"),
    ("/quit", "/exit"),
    ("/effort", "/model"),
    ("/rollback", "/undo files"),
    ("/branch", "/fork"),
    ("/tree", "/resume"),
    ("/session", "/status"),
    ("/stats", "/status"),
    ("/profile", "/usage session"),
    ("/net", "/permissions net"),
    ("/write", "/permissions write"),
    ("/approve", "/permissions approve"),
    ("/layout", "/pane"),
    ("/theme", "/settings"),
    ("/tools", "/settings"),
    ("/reasoning", "/settings"),
    ("/receipts", "/settings"),
    ("/bus", "/settings"),
    ("/modules", "/settings"),
];

const fn spec(
    name: &'static str,
    description: &'static str,
    command: SlashCommand,
    argument: Argument,
    category: Category,
) -> CommandSpec {
    CommandSpec {
        name,
        description,
        command,
        argument,
        category,
        usage: "",
    }
}
