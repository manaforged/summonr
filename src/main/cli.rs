use summonr::{Error, Result, is_endpoint};

#[derive(Debug, Default)]
pub(super) struct Args {
    pub(super) print: bool,
    pub(super) effort: Option<summonr::Effort>,
    pub(super) json: bool,
    pub(super) list: bool,
    pub(super) list_sessions: bool,
    pub(super) logs: bool,
    pub(super) doctor: bool,
    pub(super) export: bool,
    pub(super) profile: bool,
    pub(super) name: Option<String>,
    pub(super) list_workflows: bool,
    pub(super) list_modules: bool,
    pub(super) room: Option<Vec<String>>,
    pub(super) acp: bool,
    pub(super) resume: bool,
    pub(super) fork: bool,
    pub(super) trust: bool,
    pub(super) strict: bool,
    pub(super) sandbox: bool,
    pub(super) add_dir: Vec<String>,
    pub(super) network: bool,
    pub(super) plan: bool,
    pub(super) no_verify: bool,
    pub(super) no_splash: bool,
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
        network: true,
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
            "--sandbox" => args.sandbox = true,
            "--no-sandbox" => args.sandbox = false,
            "--add-dir" => {
                args.add_dir
                    .push(value(&mut input, "--add-dir", "a directory")?);
            }
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
            "profile" if args.prompt.is_empty() && !args.print => args.profile = true,
            "--name" => args.name = Some(value(&mut input, "--name", "a session name")?),
            "workflows" => args.list_workflows = true,
            "modules" => args.list_modules = true,
            "acp" if args.prompt.is_empty() && !args.print => args.acp = true,
            "room" if args.prompt.is_empty() && !args.print => {
                args.room = Some(input.by_ref().collect());
            }
            "login" => args.login = true,
            "logout" => args.logout = true,
            "--key" => args.key = true,
            "worktrees" => args.list_worktrees = true,
            "diff" if args.prompt.is_empty() && !args.print => args.diff = true,
            "--stat" => args.stat = true,
            "--gui" => args.gui = true,
            "--turn" => args.turn = Some(value(&mut input, "--turn", "a turn number or `last`")?),
            "--steps" => args.steps = Some(value(&mut input, "--steps", "a plan path")?),
            "prune" if args.list_worktrees => args.prune_worktrees = true,
            "--goal" => args.goal = Some(value(&mut input, "--goal", "an objective")?),
            "--max-turns" => {
                let turns: u32 = number(&mut input, "--max-turns")?;
                args.max_turns = Some(turns);
            }
            "--session" => args.session = Some(value(&mut input, "--session", "an id")?),
            "-e" | "--endpoint" => {
                args.endpoint = Some(value(
                    &mut input,
                    "--endpoint",
                    "a name (zai, zai-cn, kimi, grok, …)",
                )?);
            }
            "--model" => args.model = Some(value(&mut input, "--model", "a value")?),
            "-w" | "--workflow" => {
                args.workflow = Some(value(&mut input, "--workflow", "a name")?);
            }
            "--max-tokens" => args.max_tokens = Some(number(&mut input, "--max-tokens")?),
            "--max-secs" => args.max_secs = Some(number(&mut input, "--max-secs")?),
            "--effort" => {
                let level = value(&mut input, "--effort", "none, low, medium, or high")?;
                args.effort = Some(summonr::Effort::parse(&level).ok_or_else(|| {
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
    let value = input
        .next()
        .ok_or_else(|| Error::Config(format!("{option} requires {expected}")))?;
    if value.starts_with('-') {
        return Err(Error::Config(format!(
            "{option} requires {expected}, not option `{value}`"
        )));
    }
    Ok(value)
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
    for spec in summonr::command_specs() {
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
    let bin = option_env!("CARGO_BIN_NAME").unwrap_or("summonr");
    let commands = interactive();
    let endpoints = summonr::endpoint_names().map_or_else(
        |_| "the built-in endpoint names".to_string(),
        |names| names.join(", "),
    );
    println!(
        "{bin} — Summonr, a small agent harness\n\nUsage:\n  {bin} [OPTIONS]\n  {bin} grok\n  {bin} kimi -p [PROMPT]\n  {bin} -c\n  {bin} --session ID\n  {bin} -e zai -p [PROMPT]\n  {bin} sessions\n  {bin} export [ID|NAME]\n  {bin} profile [ID|NAME]\n  {bin} workflows\n  {bin} modules\n  {bin} room [serve|host codex|setup claude|setup codex]\n  {bin} acp\n  {bin} endpoints\n  {bin} login NAME [--key]\n  {bin} logout NAME\n  {bin} logs\n  {bin} doctor\n  {bin} worktrees [--json]\n  {bin} worktrees prune\n  {bin} diff [PATH] [--turn N|last] [--stat|--json]\n\nOptions:\n  -e, --endpoint NAME  A built-in or configured endpoint\n  -p, --print          Run one prompt and exit\n      --steps PLAN.md  Step mode: run each numbered step under `## Steps`, gate on its `Run `…`` check\n      --goal TEXT      Goal mode: work toward TEXT across turns; check.toml gates must pass and a judge decides done\n      --max-turns N    Optional continuation-turn limit for goal mode\n  -c, --continue       Resume the last session for this directory\n      --session ID     Resume a session by id (prefix ok) or name\n      --name TEXT      Name this session; /name in the interface\n      --json           Print JSON events as they happen\n      --model NAME     Override the endpoint model\n      --effort LEVEL   Reasoning effort for this session: none, low, medium, high\n  -w, --workflow NAME  Seat each step from [workflows.NAME] in ~/.mage/config.toml\n      --no-trust       Ignore workspace AGENTS.md\n      --strict         Allow only commands listed in policy.toml\n      --plan           Read-only workspace; inspection commands remain available\n      --net            Allow network access inside the sandbox (default)\n      --no-net         Deny network access inside the sandbox (needs --sandbox)\n      --sandbox        Contain shell commands and keep writes under the roots
      --no-sandbox     Uncontained commands and file access (the default)
      --add-dir DIR    Extra writable root (repeatable; same as [sandbox] write)\n      --no-verify      Skip the .mage/check.toml verify gate for this run\n      --no-splash      Skip the startup banner\n      --gui            Open the desktop window instead of the terminal (a `--features gui` build)\n      --here           Stay in this directory (no git worktree)\n      --worktree       Isolate this session in a git worktree\n      --max-tokens N   Session token budget; breach aborts nonzero\n      --max-secs N     Session wall-clock budget; breach aborts nonzero\n  -h, --help           Show this help\n  -V, --version        Show the version\n\nExit codes: 0 ok · 1 harness error · 2 verify failed · 3 stopped short (budget, stall, impossible, abort)\n\nInteractive:\n{commands}\n  @path attach a workspace file · ctrl+e files · ctrl+l ledger · ctrl+x diff\n  ctrl+y copy reply · ctrl+t select text · ctrl+o settings · ctrl+p hide/show pane\n  ctrl+→/← focus · ctrl+shift+←/→ resize\n\nEndpoints:\n  Available endpoints: {endpoints}\n  The first argument can be an endpoint name: `{bin} grok` is `{bin} -e grok`.\n  {bin} endpoints probes them\n  {bin} login NAME  securely ask for an API key, or use device login when supported\n  {bin} login NAME --key  force an API-key prompt instead of device login\n  {bin} logout NAME  forget the stored key or token\n\nKeys: the named endpoint environment, then ~/.mage/credentials.toml (0600; `secrets = \"keychain\"` in config.toml uses the OS store), then api_key in config.toml. MAGE_API_KEY applies only with MAGE_BASE_URL.\nKimi: KIMI_CODING_API_KEY / KIMI_API_KEY, or ~/.kimi-code/credentials/kimi-code.json.\nGrok: XAI_API_KEY.\nMuse: META_API_KEY / MODEL_API_KEY.\nAnthropic: ANTHROPIC_API_KEY. Gemini: GEMINI_API_KEY / GOOGLE_API_KEY.
OpenCode: OPENCODE_API_KEY / OPENCODE_GO_API_KEY.
Qwen needs no key. Completions seats speak OpenAI chat/completions; anthropic and gemini use native APIs. Override URLs in ~/.mage/config.toml.\nPolicy: ~/.mage/policy.toml and .mage/policy.toml both restrict access; repository rules cannot widen user permissions (see policy.example.toml).\nLong commands: run_command background true, then poll job."
    );
    println!(
        "\nDiagnostics: `summonr logs` lists recent per-run JSONL files; `summonr doctor` reports incomplete or crashed runs.\n\nSandbox: off by default. Pass --sandbox, or set `on = true` under [sandbox] in ~/.mage/config.toml, to contain commands and post-edit checks with network allowed.\n\
With the sandbox on, writes are allowed under the launch directory, temp, [sandbox] write, and --add-dir.\n\
Use --no-net to deny sandboxed sockets (it needs --sandbox), or /net off in the interface. Uncontained sessions bypass command and file policy.\n\
--plan contains commands, denies writes, and refuses uncontained shell work.\n\
--strict reapplies policy but does not restore containment. Language servers run as trusted host processes.\n\
Summonr exits with a configuration error when the selected backend is unavailable.\n\
Worktrees: sessions run in this directory. `--worktree` isolates a session in a git worktree (branch summonr/<id>), merge --ff-only on success, and remove it. `--here` is the default. `summonr worktrees` lists; `summonr worktrees prune` drops records for directories that are already gone.\n\
Diff: `summonr diff` prints the patch against the worktree base (HEAD outside a worktree); `--stat` per-file counts; `--json` the same as JSON; `--turn N` (or `last`) keeps only the files that turn of the latest session here edited; `/pane diff` shows it in the interface."
    );
    std::process::exit(0)
}

fn exit_version() -> ! {
    println!(
        "{} {} ({})",
        option_env!("CARGO_BIN_NAME").unwrap_or("summonr"),
        env!("CARGO_PKG_VERSION"),
        summonr::revision()
    );
    std::process::exit(0)
}
