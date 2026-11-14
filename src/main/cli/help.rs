use summonr_agent::Result;
use summonr_agent::table::{self, asks_help};

use super::flags::Input;

/// A noun whose help is its own usage text, and how its verbs read their words.
pub(super) struct Manual {
    pub(super) noun: &'static str,
    pub(super) usage: Option<&'static str>,
    pub(super) bare: Option<&'static str>,
    /// Verbs that accept `--json` after their own words.
    pub(super) json: &'static [&'static str],
    /// Verbs whose words after this many positionals are free text.
    pub(super) text: &'static [(&'static str, usize)],
    /// Verbs that parse but stay out of the verb list, like `askpass`.
    pub(super) internal: &'static [&'static str],
}

static MANUALS: [Manual; 5] = [
    Manual {
        noun: "cron",
        usage: Some(summonr_agent::cron::cli::USAGE),
        bare: Some("help"),
        json: &["list"],
        text: &[],
        internal: &["tick", "exec"],
    },
    Manual {
        noun: "gateway",
        usage: Some(summonr_agent::gateway::cli::USAGE),
        bare: Some("help"),
        json: &[],
        text: &[],
        internal: &[],
    },
    Manual {
        noun: "mail",
        usage: Some(summonr_agent::mail::cli::USAGE),
        bare: Some("agents"),
        json: &["agents", "send"],
        text: &[("send", 1), ("board", 2)],
        internal: &[],
    },
    Manual {
        noun: "panel",
        usage: Some(summonr_agent::panel::cli::USAGE),
        bare: None,
        json: &[],
        text: &[],
        internal: &[],
    },
    Manual {
        noun: "mcp",
        usage: None,
        bare: None,
        json: &["list", "get"],
        text: &[],
        internal: &[],
    },
];

impl Manual {
    pub(super) fn of(noun: &str) -> Option<&'static Self> {
        MANUALS.iter().find(|manual| manual.noun == noun)
    }

    /// Prints the manual and exits when a word before `--` or free text asks for help.
    pub(super) fn answer(&self, input: &mut Input) {
        let rest: Vec<String> = input.collect();
        let words: Vec<&str> = rest
            .iter()
            .map(String::as_str)
            .take_while(|word| *word != "--")
            .collect();
        if asks_help(&words[..self.before_text(&words)]) {
            exit_manual(self);
        }
        *input = rest.into_iter().peekable();
    }

    fn before_text(&self, words: &[&str]) -> usize {
        let mut plain = words
            .iter()
            .enumerate()
            .filter(|(_, word)| !word.starts_with('-'));
        let count = plain.next().and_then(|(_, verb)| {
            self.text
                .iter()
                .find(|(name, _)| name == verb)
                .map(|(_, count)| *count)
        });
        count
            .and_then(|count| plain.nth(count.saturating_sub(1)))
            .map_or(words.len(), |(index, _)| index + 1)
    }

    /// Ends a row of this noun: `--json` where `verb` takes it, any other option is an error.
    pub(super) fn seal(&self, verb: &str, json: &mut bool, input: &mut Input) -> Result<()> {
        let mut kept = Vec::new();
        for word in input.by_ref() {
            if word == "--json" && self.json.contains(&verb) {
                *json = true;
            } else if word.starts_with('-') {
                return Err(table::misuse(self.noun, &table::unknown_option(&word)));
            } else {
                kept.push(word);
            }
        }
        *input = kept.into_iter().peekable();
        Ok(())
    }

    /// Whether `verb` belongs in the list a wrong verb prints.
    pub(super) fn lists(noun: &str, verb: &str) -> bool {
        Self::of(noun).is_none_or(|manual| !manual.internal.contains(&verb))
    }
}

fn interactive() -> String {
    let mut lines = vec![String::new()];
    for spec in summonr_agent::command_specs()
        .iter()
        .filter(|spec| spec.shown())
    {
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

/// The verb sections, rendered from the command rows.
pub(super) fn verbs() -> String {
    let rows: std::collections::BTreeMap<&str, &super::commands::Command> =
        super::commands::COMMANDS
            .iter()
            .chain(super::commands::subcommands())
            .map(|row| (row.name, row))
            .collect();
    let mut out = String::new();
    for (title, names) in super::commands::SECTIONS {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push('\n');
        out.push_str(title);
        out.push(':');
        for name in names {
            let Some(row) = rows.get(name) else {
                panic!("the help section `{title}` names the unknown command `{name}`");
            };
            for line in row.help.split('\n') {
                out.push_str("\n  ");
                out.push_str(line);
            }
        }
    }
    out
}

/// The full `summonr --help` text.
pub(super) fn text() -> String {
    include_str!("../help.txt")
        .replace("{options}", &super::FLAGS.help_lines())
        .replace("{commands}", &interactive())
        .replace("{verbs}", &verbs())
        .replace("{sets}", &summonr_agent::classify::sets::NAMES.join(", "))
}

pub(super) fn exit_help(verb: Option<&str>) -> ! {
    let text = text();
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

pub(super) fn exit_version() -> ! {
    println!(
        "{} {} ({})",
        option_env!("CARGO_BIN_NAME").unwrap_or("summonr"),
        env!("CARGO_PKG_VERSION"),
        summonr_agent::revision()
    );
    std::process::exit(0)
}

pub(super) fn exit_manual(manual: &Manual) -> ! {
    match manual.usage {
        Some(text) => println!("{text}"),
        None => exit_help(Some(manual.noun)),
    }
    std::process::exit(0)
}
