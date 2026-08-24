use summoner::{Error, Result, is_endpoint};

#[derive(Debug, Default)]
pub(super) struct Args {
    pub(super) print: bool,
    pub(super) json: bool,
    pub(super) list: bool,
    pub(super) list_sessions: bool,
    pub(super) resume: bool,
    pub(super) fork: bool,
    pub(super) trust: bool,
    pub(super) strict: bool,
    pub(super) sandbox: bool,
    pub(super) no_net: bool,
    pub(super) plan: bool,
    pub(super) no_verify: bool,
    pub(super) no_splash: bool,
    pub(super) no_tui: bool,
    pub(super) max_rounds: Option<usize>,
    pub(super) max_tokens: Option<u64>,
    pub(super) max_secs: Option<u64>,
    pub(super) session: Option<String>,
    pub(super) endpoint: Option<String>,
    pub(super) model: Option<String>,
    pub(super) steps: Option<String>,
    pub(super) goal: Option<String>,
    pub(super) max_turns: Option<u32>,
    pub(super) prompt: Vec<String>,
    pub(super) here: bool,
    pub(super) worktree: bool,
    pub(super) list_worktrees: bool,
    pub(super) prune_worktrees: bool,
    pub(super) login: bool,
}

pub(super) fn parse() -> Result<Args> {
    parse_from(std::env::args().skip(1))
}

fn parse_from(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut input = args.into_iter();
    let mut args = Args::default();
    while let Some(arg) = input.next() {
        match arg.as_str() {
            "-p" | "--print" => args.print = true,
            "--json" => args.json = true,
            "--trust" => args.trust = true,
            "--strict" => args.strict = true,
            "--sandbox" => args.sandbox = true,
            "--no-net" => args.no_net = true,
            "--plan" => args.plan = true,
            "--no-verify" => args.no_verify = true,
            "--no-splash" => args.no_splash = true,
            "--no-tui" => args.no_tui = true,
            "--here" => args.here = true,
            "--worktree" => args.worktree = true,
            "-c" | "--continue" => args.resume = true,
            "--fork" => args.fork = true,
            "endpoints" => args.list = true,
            "sessions" => args.list_sessions = true,
            "login" => args.login = true,
            "worktrees" => args.list_worktrees = true,
            "--steps" => args.steps = input.next(),
            "prune" if args.list_worktrees => args.prune_worktrees = true,
            "--goal" => args.goal = Some(value(&mut input, "--goal", "an objective")?),
            "--max-turns" => args.max_turns = Some(number(&mut input, "--max-turns")?),
            "--session" => args.session = Some(value(&mut input, "--session", "an id")?),
            "-e" | "--endpoint" => {
                args.endpoint = Some(value(
                    &mut input,
                    "--endpoint",
                    "a name (glm, kimi, grok, …)",
                )?);
            }
            "--model" => args.model = Some(value(&mut input, "--model", "a value")?),
            "--max-rounds" => args.max_rounds = Some(number(&mut input, "--max-rounds")?),
            "--max-tokens" => args.max_tokens = Some(number(&mut input, "--max-tokens")?),
            "--max-secs" => args.max_secs = Some(number(&mut input, "--max-secs")?),
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
    if args.no_net && !args.sandbox {
        return Err(Error::Config("--no-net requires --sandbox".to_string()));
    }
    if args.here && args.worktree {
        return Err(Error::Config(
            "use --here or --worktree, not both".to_string(),
        ));
    }
    Ok(args)
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

fn exit_help() -> ! {
    let bin = option_env!("CARGO_BIN_NAME").unwrap_or("summon");
    println!(
        "{bin} — Summoner, a small agent harness\n\nUsage:\n  {bin} [OPTIONS]\n  {bin} grok\n  {bin} kimi -p [PROMPT]\n  {bin} -c\n  {bin} --session ID\n  {bin} -e glm -p [PROMPT]\n  {bin} sessions\n  {bin} endpoints\n  {bin} login [NAME]\n  {bin} worktrees\n  {bin} worktrees prune\n\nOptions:\n  -e, --endpoint NAME  glm, kimi, grok, qwen, copilot, openai, openrouter, anthropic, gemini, or a name from ~/.mage/config.toml\n  -p, --print          Run one prompt and exit\n      --steps PLAN.md  Step mode: run each numbered step under `## Steps`, gate on its `Run `…`` check\n      --goal TEXT      Goal mode: work toward TEXT across turns; check.toml gates must pass and a judge decides done\n      --max-turns N    Continuation turns for goal mode (default 20)\n  -c, --continue       Resume the last session for this directory\n      --session ID     Resume a session by id (prefix ok)\n      --json           Print JSON events as they happen\n      --model NAME     Override the endpoint model\n      --trust          Load workspace AGENTS.md into the system prompt\n      --strict         Allow only commands listed in policy.toml\n      --plan           Read-only mode: no edits, writes, or commands\n      --sandbox        Contain commands and post-edit checks (Seatbelt/bubblewrap)\n      --no-net         Deny network access inside --sandbox\n      --no-verify      Skip the .mage/check.toml verify gate for this run\n      --no-splash      Skip the startup banner\n      --no-tui         Line REPL instead of the full-screen interface\n      --here           Stay in this directory (no git worktree)\n      --worktree       Isolate this session in a git worktree\n      --max-rounds N   Provider rounds per turn (default 64)\n      --max-tokens N   Session token budget; breach aborts nonzero\n      --max-secs N     Session wall-clock budget; breach aborts nonzero\n  -h, --help           Show this help\n  -V, --version        Show the version\n\nExit codes: 0 ok · 1 harness error · 2 verify failed · 3 turn aborted\n\nInteractive:\n  /exit   /help   /session   /clear\n  !command   run a shell command through policy\n  @path      attach a workspace file to the prompt\n\nEndpoints:\n  glm / glm-intl / qwen / kimi / grok / copilot / openai / openrouter / anthropic / gemini  builtins; any other name is an [endpoint.NAME] row\n  The first argument can be an endpoint name: `{bin} grok` is `{bin} -e grok`.\n  {bin} endpoints lists them\n  {bin} login [NAME]  RFC 8628 device-code login for that endpoint\n\nKeys: ZAI_CODING_API_KEY / ZAI_API_KEY, or api_key in ~/.mage/config.toml.\nKimi: KIMI_CODING_API_KEY / KIMI_API_KEY, or ~/.kimi-code/credentials/kimi-code.json.\nGrok: XAI_API_KEY.\nAnthropic: ANTHROPIC_API_KEY. Gemini: GEMINI_API_KEY / GOOGLE_API_KEY.
Qwen needs no key. Completions seats speak OpenAI chat/completions; anthropic and gemini use native APIs. Override URLs in ~/.mage/config.toml.\nPolicy: ~/.mage/policy.toml then .mage/policy.toml (see policy.example.toml).\nLong commands: run_command background true, then poll job."
    );
    println!(
        "\nSandbox: --sandbox contains commands and post-edit checks; --no-net also denies sockets.\n\
Summoner exits with a configuration error when the selected backend is unavailable.\n\
Worktrees: `summon --goal` and `summon --steps` add a git worktree (branch summon/<id>), merge --ff-only on success, and remove it. `--here` stays in this directory. `--worktree` isolates an interactive session. `summon worktrees` lists; `summon worktrees prune` drops records for directories that are already gone."
    );
    std::process::exit(0)
}

fn exit_version() -> ! {
    println!(
        "{} {}",
        option_env!("CARGO_BIN_NAME").unwrap_or("summon"),
        env!("CARGO_PKG_VERSION")
    );
    std::process::exit(0)
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
