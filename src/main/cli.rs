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
    pub(super) all: bool,
    pub(super) last: Option<usize>,
    pub(super) name: Option<String>,
    pub(super) list_workflows: bool,
    pub(super) list_modules: bool,
    pub(super) room: Option<Vec<String>>,
    pub(super) tool: Option<Vec<String>>,
    pub(super) acp: bool,
    pub(super) resume: bool,
    pub(super) fork: bool,
    pub(super) trust: bool,
    pub(super) strict: bool,
    pub(super) sandbox: Option<bool>,
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
    pub(super) classify: bool,
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
    let mut input = args.into_iter().peekable();
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
                    return Err(unknown(name, "; a prompt goes after -p"));
                }
                args.print = true;
            }
            "--json" => args.json = true,
            "--trust" => args.trust = true,
            "--no-trust" => args.trust = false,
            "--strict" => args.strict = true,
            "--sandbox" => args.sandbox = Some(true),
            "--no-sandbox" => args.sandbox = Some(false),
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
            "resume" if command => match input.next_if(|next| !next.starts_with('-')) {
                Some(key) => args.session = Some(key),
                None => args.resume = true,
            },
            "logs" if command => {
                renamed("logs", "doctor --logs");
                args.logs = true;
            }
            "doctor" if command => args.doctor = true,
            "--logs" if args.doctor => args.logs = true,
            "classify" if command => args.classify = true,
            "--questions" if args.classify => {
                args.questions = Some(value(&mut input, "--questions", "JSON or @file")?);
            }
            "--noul" if args.classify => {
                args.noul
                    .push(value(&mut input, "--noul", "a yes/no question")?)
            }
            "--review" if args.classify => args.review = true,
            "--digest" if args.export => args.digest = true,
            "--patch" | "--diff" if args.classify => args.review_diff = true,
            "--set" if args.classify => {
                args.set = Some(value(&mut input, "--set", "a question set name")?)
            }
            "--eval" if args.classify => {
                args.eval = Some(value(&mut input, "--eval", "a labelled .jsonl path")?)
            }
            "--gate" if args.classify => {
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
            "--all" if args.profile => args.all = true,
            "--last" if args.profile => {
                let raw = value(&mut input, "--last", "a session count")?;
                args.last =
                    Some(raw.parse().map_err(|error| {
                        Error::Config(format!("--last wants a count: {error}"))
                    })?);
            }
            "--name" => args.name = Some(value(&mut input, "--name", "a session name")?),
            "workflows" if command => args.list_workflows = true,
            "modules" if command => args.list_modules = true,
            "install" if command => args.add = true,
            "add" if command => {
                renamed("add", "install");
                args.add = true;
            }
            "--list" if args.add => args.add_list = true,
            "acp" if command => args.acp = true,
            "room" if command => {
                args.room = Some(input.by_ref().collect());
            }
            "tool" if command => {
                args.tool = Some(input.by_ref().collect());
            }
            "login" if command => args.login = true,
            "auth" if command => {
                renamed("auth", "login");
                args.login = true;
            }
            "logout" if command => args.logout = true,
            "--key" if args.login => args.key = true,
            "worktrees" if command => args.list_worktrees = true,
            "diff" if command => args.diff = true,
            "--stat" if args.diff => args.stat = true,
            "--gui" => args.gui = true,
            "--turn" if args.diff => {
                args.turn = Some(value(&mut input, "--turn", "a turn number or `last`")?)
            }
            "--steps" => args.steps = Some(value(&mut input, "--steps", "a plan path")?),
            "prune" if args.list_worktrees => args.prune_worktrees = true,
            "--goal" => args.goal = Some(value(&mut input, "--goal", "an objective")?),
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
            "-h" | "--help" => exit_help(None),
            "help" if command => exit_help(input.next().as_deref()),
            "-V" | "--version" => exit_version(),
            "--" => {
                args.prompt.extend(input.by_ref());
                break;
            }
            option if option.starts_with('-') => {
                return Err(Error::Config(
                    SCOPED.iter().find(|(flag, _)| *flag == option).map_or_else(
                        || format!("unknown option `{option}`"),
                        |(_, verb)| format!("`{option}` works only after `summonr {verb}`"),
                    ),
                ));
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
        return Err(unknown(name, ""));
    }
    if args.here && args.worktree {
        return Err(Error::Config(
            "use --here or --worktree, not both".to_string(),
        ));
    }
    if (args.logs || args.doctor) && !args.prompt.is_empty() {
        return Err(Error::Config(
            "doctor does not accept arguments".to_string(),
        ));
    }
    Ok(args)
}

const SCOPED: [(&str, &str); 16] = [
    ("--stat", "diff"),
    ("--turn", "diff"),
    ("--all", "profile"),
    ("--last", "profile"),
    ("--list", "install"),
    ("--key", "login"),
    ("--digest", "export"),
    ("--logs", "doctor"),
    ("--questions", "classify"),
    ("--noul", "classify"),
    ("--review", "classify"),
    ("--set", "classify"),
    ("--eval", "classify"),
    ("--gate", "classify"),
    ("--patch", "classify"),
    ("--diff", "classify"),
];

fn unknown(name: &str, tail: &str) -> Error {
    let names = summonr::endpoint_names().unwrap_or_default();
    let hint = summonr::closest_endpoint(name, &names)
        .map_or_else(String::new, |near| format!("did you mean `{near}`? "));
    Error::Config(format!(
        "unknown endpoint `{name}` — {hint}`summonr endpoints` lists them{tail}"
    ))
}

fn renamed(old: &str, new: &str) {
    eprintln!("summonr: renamed: use `summonr {new}` instead of `summonr {old}`");
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
    for spec in summonr::command_specs().iter().filter(|spec| spec.shown()) {
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

fn exit_help(verb: Option<&str>) -> ! {
    let endpoints = summonr::endpoint_names().map_or_else(
        |_| "the built-in endpoint names".to_string(),
        |names| names.join(", "),
    );
    let text = include_str!("help.txt")
        .replace("{endpoints}", &endpoints)
        .replace("{commands}", &interactive())
        .replace("{sets}", &summonr::classify::sets::NAMES.join(", "));
    let lines: Vec<&str> = verb.map_or_else(Vec::new, |verb| {
        text.lines()
            .filter(|line| {
                let line = line.trim_start();
                line.strip_prefix(verb)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '|']))
                    || line.starts_with(&format!("summonr {verb} "))
            })
            .collect()
    });
    if lines.is_empty() {
        print!("{text}");
    } else {
        println!("{}", lines.join("\n"));
    }
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
