use summoner::{Error, Result, is_endpoint};

#[derive(Debug, Default)]
pub(super) struct Args {
    pub(super) print: bool,
    pub(super) effort: Option<summoner::Effort>,
    pub(super) json: bool,
    pub(super) list: bool,
    pub(super) list_sessions: bool,
    pub(super) logs: bool,
    pub(super) doctor: bool,
    pub(super) export: bool,
    pub(super) name: Option<String>,
    pub(super) list_workflows: bool,
    pub(super) resume: bool,
    pub(super) fork: bool,
    pub(super) trust: bool,
    pub(super) strict: bool,
    pub(super) no_sandbox: bool,
    pub(super) network: bool,
    pub(super) plan: bool,
    pub(super) no_verify: bool,
    pub(super) no_splash: bool,
    pub(super) max_rounds: Option<usize>,
    pub(super) max_tokens: Option<u64>,
    pub(super) max_secs: Option<u64>,
    pub(super) session: Option<String>,
    pub(super) endpoint: Option<String>,
    pub(super) model: Option<String>,
    pub(super) workflow: Option<String>,
    pub(super) steps: Option<String>,
    pub(super) goal: Option<String>,
    pub(super) max_turns: Option<u32>,
    pub(super) prompt: Vec<String>,
    pub(super) here: bool,
    pub(super) worktree: bool,
    pub(super) list_worktrees: bool,
    pub(super) gui: bool,
    pub(super) diff: bool,
    pub(super) stat: bool,
    pub(super) turn: Option<String>,
    pub(super) prune_worktrees: bool,
    pub(super) login: bool,
    pub(super) key: bool,
    pub(super) logout: bool,
}

pub(super) fn parse() -> Result<Args> {
    parse_from(std::env::args().skip(1))
}

fn parse_from(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut input = args.into_iter();
    let mut args = Args {
        trust: true,
        ..Args::default()
    };
    while let Some(arg) = input.next() {
        match arg.as_str() {
            "-p" | "--print" => {
                if let [name] = args.prompt.as_slice()
                    && args.endpoint.is_none()
                    && named(name)
                {
                    return Err(Error::Config(format!(
                        "unknown endpoint `{name}` — `summon endpoints` lists them; a prompt goes after -p"
                    )));
                }
                args.print = true;
            }
            "--json" => args.json = true,
            "--trust" => args.trust = true,
            "--no-trust" => args.trust = false,
            "--strict" => args.strict = true,
            "--sandbox" => args.no_sandbox = false,
            "--no-sandbox" => args.no_sandbox = true,
            "--net" => args.network = true,
            "--no-net" => args.network = false,
            "--plan" => args.plan = true,
            "--no-verify" => args.no_verify = true,
            "--no-splash" => args.no_splash = true,
            "--here" => args.here = true,
            "--worktree" => args.worktree = true,
            "-c" | "--continue" => args.resume = true,
            "--fork" => args.fork = true,
            "endpoints" => args.list = true,
            "sessions" => args.list_sessions = true,
            "logs" => args.logs = true,
            "doctor" => args.doctor = true,
            "export" if args.prompt.is_empty() && !args.print => args.export = true,
            "--name" => args.name = Some(value(&mut input, "--name", "a session name")?),
            "workflows" => args.list_workflows = true,
            "login" => args.login = true,
            "logout" => args.logout = true,
            "--key" => args.key = true,
            "worktrees" => args.list_worktrees = true,
            "diff" if args.prompt.is_empty() && !args.print => args.diff = true,
            "--stat" => args.stat = true,
            "--gui" => args.gui = true,
            "--turn" => args.turn = Some(value(&mut input, "--turn", "a turn number or `last`")?),
            "--steps" => args.steps = input.next(),
            "prune" if args.list_worktrees => args.prune_worktrees = true,
            "--goal" => args.goal = Some(value(&mut input, "--goal", "an objective")?),
            "--max-turns" => {
                let turns: u32 = number(&mut input, "--max-turns")?;
                if turns == 0 {
                    return Err(Error::Config("--max-turns needs at least 1".to_string()));
                }
                args.max_turns = Some(turns);
            }
            "--session" => args.session = Some(value(&mut input, "--session", "an id")?),
            "-e" | "--endpoint" => {
                args.endpoint = Some(value(
                    &mut input,
                    "--endpoint",
                    "a name (glm, kimi, grok, …)",
                )?);
            }
            "--model" => args.model = Some(value(&mut input, "--model", "a value")?),
            "-w" | "--workflow" => {
                args.workflow = Some(value(&mut input, "--workflow", "a name")?);
            }
            "--max-rounds" => args.max_rounds = Some(number(&mut input, "--max-rounds")?),
            "--max-tokens" => args.max_tokens = Some(number(&mut input, "--max-tokens")?),
            "--max-secs" => args.max_secs = Some(number(&mut input, "--max-secs")?),
            "--effort" => {
                let level = value(&mut input, "--effort", "none, low, medium, or high")?;
                args.effort = Some(summoner::Effort::parse(&level).ok_or_else(|| {
                    Error::Config(format!(
                        "--effort takes none, low, medium, or high, not `{level}`"
                    ))
                })?);
            }
            "-h" | "--help" => exit_help(),
            "-V" | "--version" => exit_version(),
            option if option.starts_with('-') => {
                return Err(Error::Config(format!("unknown option `{option}`")));
            }
            name if args.endpoint.is_none() && is_endpoint(name) => {
                args.endpoint = Some(name.to_string());
            }
            prompt => args.prompt.push(prompt.to_string()),
        }
    }
    if let [name] = args.prompt.as_slice()
        && args.endpoint.is_none()
        && (args.goal.is_some() || args.steps.is_some())
        && named(name)
    {
        return Err(Error::Config(format!(
            "unknown endpoint `{name}` — `summon endpoints` lists them"
        )));
    }
    if args.here && args.worktree {
        return Err(Error::Config(
            "use --here or --worktree, not both".to_string(),
        ));
    }
    if (args.logs || args.doctor) && !args.prompt.is_empty() {
        return Err(Error::Config(
            "logs and doctor do not accept arguments".to_string(),
        ));
    }
    Ok(args)
}

fn named(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 40
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn value(input: &mut impl Iterator<Item = String>, option: &str, expected: &str) -> Result<String> {
    input
        .next()
        .ok_or_else(|| Error::Config(format!("{option} requires {expected}")))
}

fn number<T>(input: &mut impl Iterator<Item = String>, option: &str) -> Result<T>
where
    T: std::str::FromStr,
{
    value(input, option, "a positive integer")?
        .parse()
        .map_err(|_error| Error::Config(format!("{option} requires a positive integer")))
}

fn interactive() -> String {
    let mut lines = vec![String::new()];
    for spec in summoner::command_specs() {
        let last = lines.last_mut().expect("one line");
        if last.len() + spec.name.len() + 3 > 70 {
            lines.push(String::new());
        }
        let last = lines.last_mut().expect("one line");
        if !last.is_empty() {
            last.push_str("   ");
        }
        last.push_str(spec.name);
    }
    lines
        .iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn exit_help() -> ! {
    let bin = option_env!("CARGO_BIN_NAME").unwrap_or("summon");
    let commands = interactive();
    println!(
        "{bin} — Summoner, a small agent harness\n\nUsage:\n  {bin} [OPTIONS]\n  {bin} grok\n  {bin} kimi -p [PROMPT]\n  {bin} -c\n  {bin} --session ID\n  {bin} -e glm -p [PROMPT]\n  {bin} sessions\n  {bin} export [ID|NAME]\n  {bin} workflows\n  {bin} endpoints\n  {bin} login NAME [--key]\n  {bin} logout NAME\n  {bin} logs\n  {bin} doctor\n  {bin} worktrees [--json]\n  {bin} worktrees prune\n  {bin} diff [PATH] [--turn N|last] [--stat|--json]\n\nOptions:\n  -e, --endpoint NAME  glm, kimi, grok, qwen, copilot, openai, openrouter, anthropic, gemini, or a name from ~/.mage/config.toml\n  -p, --print          Run one prompt and exit\n      --steps PLAN.md  Step mode: run each numbered step under `## Steps`, gate on its `Run `…`` check\n      --goal TEXT      Goal mode: work toward TEXT across turns; check.toml gates must pass and a judge decides done\n      --max-turns N    Continuation turns for goal mode (default 20)\n  -c, --continue       Resume the last session for this directory\n      --session ID     Resume a session by id (prefix ok) or name\n      --name TEXT      Name this session; /name in the interface\n      --json           Print JSON events as they happen\n      --model NAME     Override the endpoint model\n      --effort LEVEL   Reasoning effort for this session: none, low, medium, high\n  -w, --workflow NAME  Seat each step from [workflows.NAME] in ~/.mage/config.toml\n      --no-trust       Ignore workspace AGENTS.md\n      --strict         Allow only commands listed in policy.toml\n      --plan           Read-only mode: no edits, writes, or commands\n      --net            Allow network access inside the sandbox\n      --no-sandbox     Disable command containment (unsafe)\n      --no-verify      Skip the .mage/check.toml verify gate for this run\n      --no-splash      Skip the startup banner\n      --gui            Open the desktop window instead of the terminal (a `--features gui` build)\n      --here           Stay in this directory (no git worktree)\n      --worktree       Isolate this session in a git worktree\n      --max-rounds N   Provider rounds per turn (default 64)\n      --max-tokens N   Session token budget; breach aborts nonzero\n      --max-secs N     Session wall-clock budget; breach aborts nonzero\n  -h, --help           Show this help\n  -V, --version        Show the version\n\nExit codes: 0 ok · 1 harness error · 2 verify failed · 3 stopped short (budget, stall, impossible, abort)\n\nInteractive:\n{commands}\n  @path      attach a workspace file to the prompt\n  ctrl+e files · ctrl+l ledger · ctrl+p hide/show the pane · ctrl+→/← focus · ctrl+shift+←/→ resize\n\nEndpoints:\n  glm / glm-intl / qwen / kimi / grok / copilot / openai / openrouter / anthropic / gemini  builtins; any other name is an [endpoint.NAME] row\n  The first argument can be an endpoint name: `{bin} grok` is `{bin} -e grok`.\n  {bin} endpoints lists them\n  {bin} login NAME  RFC 8628 device-code login for that endpoint\n  {bin} login NAME --key  store an API key (prompts; echo off)\n  {bin} logout NAME  forget the stored key or token\n\nKeys: the environment (ZAI_CODING_API_KEY / ZAI_API_KEY, …), then ~/.mage/credentials.toml (0600; `secrets = \"keychain\"` in config.toml uses the OS store), then api_key in config.toml.\nKimi: KIMI_CODING_API_KEY / KIMI_API_KEY, or ~/.kimi-code/credentials/kimi-code.json.\nGrok: XAI_API_KEY.\nAnthropic: ANTHROPIC_API_KEY. Gemini: GEMINI_API_KEY / GOOGLE_API_KEY.
Qwen needs no key. Completions seats speak OpenAI chat/completions; anthropic and gemini use native APIs. Override URLs in ~/.mage/config.toml.\nPolicy: ~/.mage/policy.toml then .mage/policy.toml (see policy.example.toml).\nLong commands: run_command background true, then poll job."
    );
    println!(
        "\nDiagnostics: `summon logs` lists recent per-run JSONL files; `summon doctor` reports incomplete or crashed runs.\n\nSandbox: commands and post-edit checks are contained by default, with network denied.\n\
Use --net to allow sockets or --no-sandbox to disable containment.\n\
Summoner exits with a configuration error when the selected backend is unavailable.\n\
Worktrees: `summon --goal` and `summon --steps` add a git worktree (branch summon/<id>), merge --ff-only on success, and remove it. `--here` stays in this directory. `--worktree` isolates an interactive session. `summon worktrees` lists; `summon worktrees prune` drops records for directories that are already gone.\n\
Diff: `summon diff` prints the patch against the worktree base (HEAD outside a worktree); `--stat` per-file counts; `--json` the same as JSON; `--turn N` (or `last`) keeps only the files that turn of the latest session here edited; `/pane diff` shows it in the interface."
    );
    std::process::exit(0)
}

fn exit_version() -> ! {
    println!(
        "{} {} ({})",
        option_env!("CARGO_BIN_NAME").unwrap_or("summon"),
        env!("CARGO_PKG_VERSION"),
        summoner::revision()
    );
    std::process::exit(0)
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
