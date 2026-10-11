use summonr::{Error, Result, is_endpoint};

use flags::{COMMANDS, Input, SCOPED, SWITCHES, VALUES};

#[path = "cli/flags.rs"]
mod flags;
#[path = "cli/mcp.rs"]
mod mcp;

pub(super) use mcp::McpCommand;

#[derive(Clone, Debug, Default)]
pub(super) struct Args {
    pub(super) print: bool,
    pub(super) effort: Option<summonr::Effort>,
    pub(super) json: bool,
    pub(super) list: bool,
    pub(super) list_sessions: bool,
    pub(super) grep: Option<String>,
    pub(super) logs: bool,
    pub(super) doctor: bool,
    pub(super) export: bool,
    pub(super) profile: bool,
    pub(super) all: bool,
    pub(super) last: Option<usize>,
    pub(super) name: Option<String>,
    pub(super) list_workflows: bool,
    pub(super) list_modules: bool,
    pub(super) mail: Option<Vec<String>>,
    pub(super) cron: Option<Vec<String>>,
    pub(super) tool: Option<Vec<String>>,
    pub(super) mcp: Option<McpCommand>,
    pub(super) acp: bool,
    pub(super) resume: bool,
    pub(super) fork: bool,
    pub(super) trust: bool,
    pub(super) strict: bool,
    pub(super) sandbox: Option<bool>,
    pub(super) add_dir: Vec<String>,
    pub(super) network: bool,
    pub(super) no_verify: bool,
    pub(super) no_splash: bool,
    pub(super) inline: bool,
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
    pub(super) url: Option<String>,
    pub(super) default: bool,
    pub(super) logout: bool,
    pub(super) remove: bool,
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
    pub(super) uninstall: bool,
}

pub(super) fn parse() -> Result<Args> {
    parse_from(std::env::args().skip(1))
}

enum Step {
    Next,
    Endpoint,
    Rest,
}

fn parse_from(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut input: Input = args.into_iter().collect::<Vec<_>>().into_iter().peekable();
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
        match args.step(&mut input, &arg, command)? {
            Step::Next => {}
            Step::Endpoint => {
                args.endpoint = Some(arg);
                command_position = command;
            }
            Step::Rest => {
                args.prompt.extend(input.by_ref());
                break;
            }
        }
    }
    args.check()?;
    Ok(args)
}

impl Args {
    fn step(&mut self, input: &mut Input, arg: &str, command: bool) -> Result<Step> {
        if let Some((_, set)) = SWITCHES.iter().find(|(names, _)| names.contains(&arg)) {
            set(self);
        } else if let Some(valued) = VALUES
            .iter()
            .find(|valued| valued.option == arg || valued.short == Some(arg))
        {
            (valued.set)(self, value(input, valued.option, valued.expected)?)?;
        } else if let Some((_, run)) = COMMANDS.iter().find(|(name, _)| command && *name == arg) {
            run(self, input)?;
        } else if let Some((.., run)) = SCOPED
            .iter()
            .find(|(flag, _, active, _)| *flag == arg && active(self))
        {
            run(self, input)?;
        } else {
            return self.rest(arg, command);
        }
        Ok(Step::Next)
    }

    fn rest(&mut self, arg: &str, command: bool) -> Result<Step> {
        match arg {
            "-p" | "--print" => self.print().map(|()| Step::Next),
            "-h" | "--help" => exit_help(self.verb()),
            "-V" | "--version" => exit_version(),
            "--" => Ok(Step::Rest),
            option if option.starts_with('-') => Err(misplaced(option)),
            name => self.positional(name, command),
        }
    }

    fn positional(&mut self, name: &str, command: bool) -> Result<Step> {
        if self.endpoint_slot(command) && is_endpoint(name) {
            return Ok(Step::Endpoint);
        }
        if self.login && self.endpoint.is_none() {
            self.endpoint = Some(name.to_string());
            return Ok(Step::Next);
        }
        if self.logout && self.endpoint.is_none() {
            return Err(unknown(name, unlisted(self.login)));
        }
        self.prompt.push(name.to_string());
        Ok(Step::Next)
    }

    fn print(&mut self) -> Result<()> {
        if let [name] = self.prompt.as_slice()
            && self.endpoint.is_none()
            && named(name)
        {
            return Err(unknown(name, unlisted(true)));
        }
        self.print = true;
        Ok(())
    }

    fn verb(&self) -> Option<&'static str> {
        SCOPED
            .iter()
            .find(|(_, _, active, _)| active(self))
            .map(|(_, verb, ..)| *verb)
    }

    fn endpoint_slot(&self, command: bool) -> bool {
        self.endpoint.is_none() && (command || self.login || self.logout)
    }

    fn check_names(&self) -> Result<()> {
        if self.login
            && self.url.is_none()
            && let Some(name) = self.endpoint.as_deref()
            && !is_endpoint(name)
        {
            return Err(unknown(name, unlisted(true)));
        }
        if let [name] = self.prompt.as_slice()
            && self.endpoint.is_none()
            && (self.goal.is_some() || self.steps.is_some())
            && named(name)
        {
            return Err(unknown(name, ""));
        }
        Ok(())
    }

    fn check(&self) -> Result<()> {
        if self.grep.is_some() && !self.list_sessions {
            return Err(Error::Config(
                "--grep works with `summonr sessions`".to_string(),
            ));
        }
        if self.here && self.worktree {
            return Err(Error::Config(
                "use --here or --worktree, not both".to_string(),
            ));
        }
        if let Some(mode) = [
            ("--goal", self.goal.is_some()),
            ("--steps", self.steps.is_some()),
        ]
        .into_iter()
        .find_map(|(mode, on)| on.then_some(mode))
            && let Some(extra) = self.prompt.first()
        {
            return Err(Error::Config(format!(
                "{mode} takes no prompt; put `{extra}` in the objective or the plan"
            )));
        }
        self.check_names()?;
        self.check_arity()
    }

    fn check_arity(&self) -> Result<()> {
        let arity = [
            (self.list, "endpoints", 0),
            (self.list_sessions, "sessions", 0),
            (self.list_workflows, "workflows", 0),
            (self.list_modules, "modules", 0),
            (self.acp, "acp", 0),
            (self.list_worktrees, "worktrees", 0),
            (self.logs || self.doctor, "doctor", 0),
            (self.logout, "logout", 0),
            (self.export, "export", 1),
            (self.profile, "profile", 1),
            (self.diff, "diff", 1),
            (self.add, "install", 1),
            (self.uninstall, "uninstall", 1),
        ];
        let Some((_, verb, most)) = arity.into_iter().find(|(on, ..)| *on) else {
            return Ok(());
        };
        match self.prompt.get(most) {
            Some(extra) if most == 0 => Err(Error::Config(format!(
                "`summonr {verb}` takes no arguments; got `{extra}`"
            ))),
            Some(extra) => Err(Error::Config(format!(
                "`summonr {verb}` takes one argument; got extra `{extra}`"
            ))),
            None => Ok(()),
        }
    }
}

fn misplaced(option: &str) -> Error {
    Error::Config(
        SCOPED
            .iter()
            .find(|(flag, ..)| *flag == option)
            .map_or_else(
                || format!("unknown option `{option}`"),
                |(_, verb, ..)| format!("`{option}` works only after `summonr {verb}`"),
            ),
    )
}

fn unlisted(login: bool) -> &'static str {
    if login {
        "; add a server with `summonr login NAME --url URL [--model NAME]`"
    } else {
        ""
    }
}

fn unknown(name: &str, tail: &str) -> Error {
    let names = summonr::endpoint_names().unwrap_or_default();
    let hint = summonr::closest_endpoint(name, &names, summonr::EndpointNear::Typo)
        .map_or_else(String::new, |near| format!("did you mean `{near}`? "));
    Error::Config(format!(
        "unknown endpoint `{name}`: {hint}`summonr endpoints` lists them{tail}"
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

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
