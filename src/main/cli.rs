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
    pub(super) decide: bool,
    pub(super) questions: Option<String>,
    pub(super) noul: Vec<String>,
    pub(super) review: bool,
    pub(super) set: Option<String>,
    pub(super) eval: Option<String>,
    pub(super) gate: Option<f64>,
    pub(super) review_diff: bool,
    pub(super) digest: bool,
    pub(super) add: bool,
    pub(super) add_list: bool,
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
    let mut command_position = true;
    while let Some(arg) = input.next() {
        let command = command_position && !args.print;
        if !arg.starts_with('-') {
            command_position = false;
        }
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
            "endpoints" if command => args.list = true,
            "sessions" if command => args.list_sessions = true,
            "logs" if command => args.logs = true,
            "doctor" if command => args.doctor = true,
            "infer" if command => args.decide = true,
            "decide" if command => {
                return Err(Error::Config(
                    "use `summonr infer`; `decide` was removed".to_string(),
                ));
            }
            "--questions" => {
                args.questions = Some(value(&mut input, "--questions", "JSON or @file")?);
            }
            "--noul" => args
                .noul
                .push(value(&mut input, "--noul", "a yes/no question")?),
            "--review" => args.review = true,
            "--digest" => args.digest = true,
            "--diff" if args.decide => args.review_diff = true,
            "--set" => args.set = Some(value(&mut input, "--set", "a question set name")?),
            "--eval" => args.eval = Some(value(&mut input, "--eval", "a labelled .jsonl path")?),
            "--gate" => {
                let raw = value(&mut input, "--gate", "a confidence from 0 to 1")?;
                let floor: f64 = raw.parse().map_err(|error| {
                    Error::Config(format!("--gate wants a number from 0 to 1: {error}"))
                })?;
                if !(0.0..=1.0).contains(&floor) {
                    return Err(Error::Config(
                        "--gate wants a number from 0 to 1".to_string(),
                    ));
                }
                args.gate = Some(floor);
            }
            "export" if command => args.export = true,
            "profile" if command => args.profile = true,
            "--name" => args.name = Some(value(&mut input, "--name", "a session name")?),
            "workflows" if command => args.list_workflows = true,
            "modules" if command => args.list_modules = true,
            "add" if command => args.add = true,
            "--list" if args.add => args.add_list = true,
            "acp" if command => args.acp = true,
            "room" if command => {
                args.room = Some(input.by_ref().collect());
            }
            "login" | "auth" if command => args.login = true,
            "logout" if command => args.logout = true,
            "--key" => args.key = true,
            "worktrees" if command => args.list_worktrees = true,
            "diff" if command => args.diff = true,
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
            "--" => {
                args.prompt.extend(input.by_ref());
                break;
            }
            option if option.starts_with('-') => {
                return Err(Error::Config(format!("unknown option `{option}`")));
            }
            name if args.endpoint.is_none()
                && (command || args.login || args.logout)
                && is_endpoint(name) =>
            {
                args.endpoint = Some(name.to_string());
                command_position = command;
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
    let sets = summonr::decisions::sets::NAMES.join(", ");
    let endpoints = summonr::endpoint_names().map_or_else(
        |_| "the built-in endpoint names".to_string(),
        |names| names.join(", "),
    );
    println!(
        "{bin} — Summonr, a small agent harness\n\nUsage:\n  {bin} [OPTIONS]\n  {bin} grok\n  {bin} kimi -p [PROMPT]\n  {bin} -c\n  {bin} --session ID\n  {bin} -e zai -p [PROMPT]\n  {bin} sessions\n  {bin} export [ID|NAME]\n  {bin} profile [ID|NAME]\n  {bin} workflows\n  {bin} modules\n  {bin} add PATH\n  {bin} add --list\n  {bin} room [serve|host codex|setup claude|setup codex]\n  {bin} acp\n  {bin} endpoints\n  {bin} infer --set NAME [STATE]\n  {bin} infer --questions @FILE [STATE]\n  {bin} login\n  {bin} login NAME [--key]\n  {bin} auth\n  {bin} logout NAME\n  {bin} logs\n  {bin} doctor\n  {bin} worktrees [--json]\n  {bin} worktrees prune\n  {bin} diff [PATH] [--turn N|last] [--stat|--json]\n\nOptions:\n  -e, --endpoint NAME  A built-in or configured endpoint\n  -p, --print          Run one prompt and exit\n      --steps PLAN.md  Step mode: run each numbered step under `## Steps`, gate on its `Run `…`` check\n      --goal TEXT      Goal mode: work toward TEXT across turns; check.toml gates must pass and a judge decides done\n      --max-turns N    Optional continuation-turn limit for goal mode\n  -c, --continue       Resume the last session for this directory\n      --session ID     Resume a session by id (prefix ok) or name\n      --name TEXT      Name this session; /name in the interface\n      --json           Print JSON events as they happen\n      --model NAME     Override the endpoint model\n      --effort LEVEL   Reasoning effort for this session: none, low, medium, high\n  -w, --workflow NAME  Seat each step from [workflows.NAME] in ~/.summonr/config.toml\n      --no-trust       Ignore workspace AGENTS.md\n      --strict         Allow only commands listed in policy.toml\n      --plan           Read-only workspace; inspection commands remain available\n      --net            Allow network access inside the sandbox (default)\n      --no-net         Deny network access inside the sandbox (needs --sandbox)\n      --sandbox        Contain shell commands and keep writes under the roots
      --no-sandbox     Uncontained commands and file access (the default)
      --add-dir DIR    Extra writable root (repeatable; same as [sandbox] write)\n      --no-verify      Skip the .summonr/check.toml verify gate for this run\n      --no-splash      Skip the startup banner\n      --gui            Open the desktop window instead of the terminal (a `--features gui` build)\n      --here           Stay in this directory (no git worktree)\n      --worktree       Isolate this session in a git worktree\n      --max-tokens N   Session token budget; breach aborts nonzero\n      --max-secs N     Session wall-clock budget; breach aborts nonzero\n  -h, --help           Show this help\n  -V, --version        Show the version\n\nExit codes: 0 ok · 1 harness error · 2 verify failed · 3 stopped short (budget, stall, impossible, abort)\n\nInteractive:\n{commands}\n  @path attach a workspace file · ctrl+e files · ctrl+l ledger · ctrl+x diff\n  ctrl+y copy reply · ctrl+t select text · ctrl+o settings · ctrl+p hide/show pane\n  ctrl+→/← focus · ctrl+shift+←/→ resize\n\nEndpoints:\n  Available endpoints: {endpoints}\n  The first argument can be an endpoint name: `{bin} grok` is `{bin} -e grok`.\n  {bin} endpoints probes them\n  {bin} login / {bin} auth  list API keys, coding plans, and decisions seats\n  {bin} login NAME  ask for an API key, or run the plan sign-in when supported\n  {bin} login NAME --key  force an API-key prompt instead of signing in\n  {bin} logout NAME  forget the stored key or token\n\nKeys: the named endpoint environment, then ~/.summonr/credentials.toml (0600; `secrets = \"keychain\"` in config.toml uses the OS store), then api_key in config.toml. SUMMONR_API_KEY applies only with SUMMONR_BASE_URL.\nKimi: KIMI_CODING_API_KEY / KIMI_API_KEY, or ~/.kimi-code/credentials/kimi-code.json.\nGrok: XAI_API_KEY.\nMuse: META_API_KEY / MODEL_API_KEY.\nAnthropic: ANTHROPIC_API_KEY. Gemini: GEMINI_API_KEY / GOOGLE_API_KEY.
TypeSafe / Jev: TYPESAFE_API_KEY, or `{bin} login jev`. OpenRouter is a chat seat: OPENROUTER_API_KEY, or `{bin} login openrouter`, then `/model`.
OpenCode: OPENCODE_API_KEY / OPENCODE_GO_API_KEY.
Qwen needs no key. Completions seats speak OpenAI chat/completions; anthropic and gemini use native APIs. Override URLs in ~/.summonr/config.toml.\nPolicy: ~/.summonr/policy.toml and .summonr/policy.toml both restrict access; repository rules cannot widen user permissions (see policy.example.toml).\nLong commands: run_command background true, then poll job."
    );
    println!(
        "\nInference:\n  {bin} infer -e jev --set errors [STATE]\n  {bin} infer --noul QUESTION [STATE]\n  {bin} infer --diff\n  {bin} infer --review --session ID\n  {bin} infer --set NAME --eval FILE\n\n  --set NAME        Built-in questions: {sets}\n  --questions JSON  Custom questions as JSON or @file\n  --noul QUESTION   A yes/no question; repeat to ask several\n  --json            Print typed answers, probabilities, and usage\n  --gate NUMBER     Confidence threshold for diff ranking and evaluation\n\nState comes from arguments or stdin. Use -e to select an inference endpoint."
    );
    println!(
        "\nDiagnostics: `summonr logs` lists recent per-run JSONL files; `summonr doctor` reports incomplete or crashed runs.\n\nSandbox: off by default. Pass --sandbox, or set `on = true` under [sandbox] in ~/.summonr/config.toml, to contain commands and post-edit checks with network allowed.\n\
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
