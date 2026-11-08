use super::flags::{Run, verb};
use super::{Args, Verb, exit_help, mcp};

pub(super) struct Command {
    pub(super) name: &'static str,
    pub(super) most: Option<usize>,
    pub(super) run: Run,
    /// The help line as printed, or empty to stay out of `summonr --help`.
    pub(super) help: &'static str,
}

/// Help sections claim command rows by name, in print order.
pub(super) const SECTIONS: [(&str, &[&str]); 4] = [
    (
        "Sessions",
        &[
            "session resume",
            "session list",
            "session search",
            "session export",
            "session profile",
            "session diff",
            "session children",
            "worktree list",
        ],
    ),
    (
        "Providers",
        &[
            "endpoint list",
            "endpoint usage",
            "endpoint add",
            "endpoint remove",
            "login",
            "logout",
        ],
    ),
    (
        "Setup",
        &[
            "workflow list",
            "module list",
            "mcp",
            "panel",
            "plugin install",
            "plugin list",
            "plugin remove",
            "update",
            "doctor",
        ],
    ),
    (
        "Integrations",
        &["acp", "mail", "tool", "classify", "cron", "gateway"],
    ),
];

pub(super) const COMMANDS: [Command; 19] = [
    Command {
        name: "session",
        most: None,
        run: |args, input| args.noun("session", input),
        help: "",
    },
    Command {
        name: "endpoint",
        most: None,
        run: |args, input| args.noun("endpoint", input),
        help: "",
    },
    Command {
        name: "worktree",
        most: None,
        run: |args, input| args.noun("worktree", input),
        help: "",
    },
    Command {
        name: "module",
        most: None,
        run: |args, input| args.noun("module", input),
        help: "",
    },
    Command {
        name: "workflow",
        most: None,
        run: |args, input| args.noun("workflow", input),
        help: "",
    },
    Command {
        name: "plugin",
        most: None,
        run: |args, input| args.noun("plugin", input),
        help: "",
    },
    Command {
        name: "doctor",
        most: Some(0),
        run: |args, _| verb(args, Verb::Doctor),
        help: "doctor [--logs|--report]    report crashed runs; --logs lists run logs;\n                            --report prints the last crash to paste into an issue",
    },
    Command {
        name: "classify",
        most: None,
        run: |args, _| verb(args, Verb::Classify),
        help: "classify --set NAME [STATE] answer typed questions about text (below)",
    },
    Command {
        name: "update",
        most: Some(0),
        run: |args, _| verb(args, Verb::Update),
        help: "update                      replace this binary with the latest release",
    },
    Command {
        name: "acp",
        most: Some(0),
        run: |args, _| verb(args, Verb::Acp),
        help: "acp                         serve the Agent Client Protocol on stdio",
    },
    Command {
        name: "mail",
        most: None,
        run: |args, input| verb(args, Verb::Mail(input.by_ref().collect())),
        help: "mail                        peer mail between agent sessions: agents, send, inbox, board",
    },
    Command {
        name: "cron",
        most: None,
        run: |args, input| verb(args, Verb::Cron(input.by_ref().collect())),
        help: "cron add NAME ...           run a prompt or goal on a schedule; cron --help",
    },
    Command {
        name: "panel",
        most: None,
        run: |args, input| verb(args, Verb::Panel(input.by_ref().collect())),
        help: "panel list|check NAME       list custom panels, or run one and validate its output",
    },
    Command {
        name: "gateway",
        most: None,
        run: |args, input| verb(args, Verb::Gateway(input.by_ref().collect())),
        help: "gateway telegram|webhook    start work from a Telegram chat or a web request; gateway --help",
    },
    Command {
        name: "tool",
        most: None,
        run: |args, input| {
            input.next_if(|word| word == "run");
            verb(args, Verb::Tool(input.by_ref().collect()))
        },
        help: "tool run NAME [JSON]        run one tool and print the result",
    },
    Command {
        name: "mcp",
        most: None,
        run: |args, input| verb(args, Verb::Mcp(mcp::parse(input)?)),
        help: "mcp list [--json]           list MCP servers, their state, and tool counts\nmcp get NAME [--json]       show one MCP server and its tools\nmcp add NAME -- CMD [ARGS]  add a stdio server (--env K=V), or --url URL (--header K=V)\nmcp remove NAME             remove an MCP server from ~/.summonr/config.toml",
    },
    Command {
        name: "login",
        most: None,
        run: |args, _| verb(args, Verb::Login),
        help: "login NAME [--key]          sign in to NAME or store its key\nlogin NAME --default        start on NAME when no endpoint is named (--model NAME sets its model)",
    },
    Command {
        name: "logout",
        most: Some(0),
        run: |args, _| verb(args, Verb::Logout),
        help: "logout NAME                 forget the stored key or token",
    },
    Command {
        name: "help",
        most: None,
        run: |_, input| exit_help(input.next().as_deref()),
        help: "",
    },
];

pub(super) const SUBCOMMANDS: [Command; 18] = [
    Command {
        name: "session list",
        most: Some(0),
        run: |args, _| verb(args, Verb::Sessions),
        help: "session list                list sessions",
    },
    Command {
        name: "session search",
        most: Some(0),
        run: Args::search,
        help: "session search TEXT         search past sessions",
    },
    Command {
        name: "session resume",
        most: None,
        run: Args::resume,
        help: "session resume [ID|NAME]    resume a session; bare resumes the last one here",
    },
    Command {
        name: "session export",
        most: Some(1),
        run: |args, _| verb(args, Verb::Export),
        help: "session export [ID|NAME]    markdown, --digest for classify, or --json for complete audit records",
    },
    Command {
        name: "session profile",
        most: Some(1),
        run: |args, _| verb(args, Verb::Profile),
        help: "session profile [ID|NAME]   time and tokens of a session (--all [--last N])",
    },
    Command {
        name: "session diff",
        most: Some(1),
        run: |args, _| verb(args, Verb::Diff),
        help: "session diff [PATH]         changes against the base (--stat, --json, --turn N|last, --child ID)",
    },
    Command {
        name: "session children",
        most: Some(1),
        run: |args, _| verb(args, Verb::Children),
        help: "session children [ID|NAME]  the children a lead session started (--json)",
    },
    Command {
        name: "endpoint list",
        most: Some(0),
        run: |args, _| verb(args, Verb::Endpoints),
        help: "endpoint list               list endpoints, their models, and each key state (--all, --check)",
    },
    Command {
        name: "endpoint usage",
        most: Some(0),
        run: |args, _| verb(args, Verb::Plans),
        help: "endpoint usage              show each plan's usage windows and when they reset (--json)",
    },
    Command {
        name: "endpoint add",
        most: Some(0),
        run: |args, _| {
            args.add = true;
            verb(args, Verb::Login)
        },
        help: "endpoint add NAME --url URL add your own OpenAI-compatible server (--model NAME to pick one)\nendpoint add NAME --model M change the model of a server you added",
    },
    Command {
        name: "endpoint remove",
        most: Some(0),
        run: |args, _| {
            args.remove = true;
            verb(args, Verb::Logout)
        },
        help: "endpoint remove NAME        delete an endpoint you added",
    },
    Command {
        name: "worktree list",
        most: Some(0),
        run: |args, _| verb(args, Verb::Worktrees),
        help: "worktree list|prune         list summonr worktrees, or drop gone ones (--json)",
    },
    Command {
        name: "worktree prune",
        most: Some(0),
        run: |args, _| {
            args.prune_worktrees = true;
            verb(args, Verb::Worktrees)
        },
        help: "",
    },
    Command {
        name: "module list",
        most: Some(0),
        run: |args, _| verb(args, Verb::Modules),
        help: "module list                 list the built-in modules",
    },
    Command {
        name: "workflow list",
        most: Some(0),
        run: |args, _| verb(args, Verb::Workflows),
        help: "workflow list               list saved workflows",
    },
    Command {
        name: "plugin install",
        most: Some(1),
        run: |args, _| verb(args, Verb::Install),
        help: "plugin install PATH         install a directory of extras",
    },
    Command {
        name: "plugin list",
        most: Some(0),
        run: |args, _| {
            args.add_list = true;
            verb(args, Verb::Install)
        },
        help: "plugin list                 list installed plugins",
    },
    Command {
        name: "plugin remove",
        most: Some(1),
        run: |args, _| verb(args, Verb::Uninstall),
        help: "plugin remove NAME          remove an installed plugin",
    },
];
