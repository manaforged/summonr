use summonr::{Error, Result, is_endpoint};

use flags::{COMMANDS, CONFLICTS, FLAGS, Input};

#[path = "cli/flags.rs"]
mod flags;
#[path = "cli/help.rs"]
mod help;
#[path = "cli/mcp.rs"]
mod mcp;
#[path = "cli/typo.rs"]
mod typo;

use help::exit_help;
pub(super) use mcp::McpCommand;

#[derive(Clone, Debug)]
pub(super) enum Verb {
    Endpoints,
    Sessions,
    Doctor,
    Classify,
    Export,
    Profile,
    Workflows,
    Modules,
    Install,
    Uninstall,
    Update,
    Acp,
    Mail(Vec<String>),
    Cron(Vec<String>),
    Gateway(Vec<String>),
    Tool(Vec<String>),
    Mcp(McpCommand),
    Login,
    Logout,
    Worktrees,
    Diff,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Args {
    pub(super) verb: Option<Verb>,
    pub(super) print: bool,
    pub(super) effort: Option<summonr::Effort>,
    pub(super) json: bool,
    pub(super) live: bool,
    pub(super) grep: Option<String>,
    pub(super) logs: bool,
    pub(super) report: bool,
    pub(super) all: bool,
    pub(super) last: Option<usize>,
    pub(super) name: Option<String>,
    pub(super) resume: bool,
    pub(super) fork: bool,
    pub(super) trust: bool,
    pub(super) strict: bool,
    pub(super) add_dir: Vec<String>,
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
    pub(super) gui: bool,
    pub(super) stat: bool,
    pub(super) turn: Option<String>,
    pub(super) prune_worktrees: bool,
    pub(super) key: bool,
    pub(super) url: Option<String>,
    pub(super) default: bool,
    pub(super) remove: bool,
    pub(super) questions: Option<String>,
    pub(super) noul: Vec<String>,
    pub(super) review: bool,
    pub(super) set: Option<String>,
    pub(super) eval: Option<String>,
    pub(super) gate: Option<f64>,
    pub(super) review_diff: bool,
    pub(super) digest: bool,
    pub(super) add_list: bool,
    pub(super) arity: Option<(&'static str, usize)>,
}

pub(super) fn parse() -> Result<Args> {
    parse_from(std::env::args().skip(1))
}

enum Step {
    Next,
    Endpoint,
}

fn parse_from(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let words: Vec<String> = args.into_iter().collect();
    typo::refuse(&words)?;
    let mut input: Input = words.into_iter().peekable();
    let mut args = Args {
        trust: true,
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
        }
    }
    args.check()?;
    Ok(args)
}

impl Args {
    fn step(&mut self, input: &mut Input, arg: &str, command: bool) -> Result<Step> {
        if let Some(row) = FLAGS.find(arg, self) {
            FLAGS.apply(row, arg, self, input)?;
        } else if let Some((name, most, run)) =
            COMMANDS.iter().find(|(name, ..)| command && *name == arg)
        {
            self.arity = most.map(|most| (*name, most));
            run(self, input)?;
        } else if arg.starts_with('-') {
            return Err(misplaced(arg));
        } else {
            return self.positional(arg, command);
        }
        Ok(Step::Next)
    }

    fn positional(&mut self, name: &str, command: bool) -> Result<Step> {
        if self.endpoint_slot(command) && is_endpoint(name) {
            return Ok(Step::Endpoint);
        }
        if self.endpoint.is_none() {
            match self.verb {
                Some(Verb::Login) => {
                    self.endpoint = Some(name.to_string());
                    return Ok(Step::Next);
                }
                Some(Verb::Logout) => return Err(unknown(name, "")),
                _ => {}
            }
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
        FLAGS.scoped(self)
    }

    fn endpoint_slot(&self, command: bool) -> bool {
        self.endpoint.is_none()
            && (command || matches!(self.verb, Some(Verb::Login | Verb::Logout)))
    }

    fn check_names(&self) -> Result<()> {
        if matches!(self.verb, Some(Verb::Login))
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
        if let Some((_, message)) = CONFLICTS.iter().find(|(broken, _)| broken(self)) {
            return Err(Error::Config((*message).to_string()));
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
        let Some((verb, most)) = self.arity else {
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
    let verbs: Vec<String> = FLAGS
        .verbs(option)
        .iter()
        .map(|verb| format!("`summonr {verb}`"))
        .collect();
    Error::Config(if verbs.is_empty() {
        format!("unknown option `{option}`")
    } else {
        format!("`{option}` works only after {}", verbs.join(" or "))
    })
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

fn named(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 40
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
