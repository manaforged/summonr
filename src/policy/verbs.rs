use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Cond {
    Always,
    Any(BTreeSet<String>),
    All(BTreeSet<String>),
    Prefix(Vec<String>),
    Short(char),
    First(String),
    Runs {
        flags: BTreeSet<String>,
        verbs: BTreeSet<String>,
    },
}

impl Cond {
    fn hit(&self, args: &[String]) -> bool {
        match self {
            Cond::Always => true,
            Cond::Any(names) => args.iter().any(|arg| names.contains(arg)),
            // complexity: bounded by the words of one command
            Cond::All(names) => names.iter().all(|name| args.contains(name)),
            Cond::Prefix(names) => args
                .iter()
                // complexity: bounded by the prefixes of one rule
                .any(|arg| names.iter().any(|name| arg.starts_with(name.as_str()))),
            Cond::Short(flag) => args
                .iter()
                // complexity: bounded by the characters of one short flag group
                .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains(*flag)),
            Cond::First(name) => args.first() == Some(name),
            Cond::Runs { flags, verbs } => args.windows(2).any(|pair| {
                flags.contains(&pair[0])
                    && verbs.contains(pair[1].rsplit('/').next().unwrap_or(&pair[1]))
            }),
        }
    }
}

pub(crate) fn any_hit(conds: &[Cond], args: &[String]) -> bool {
    conds.iter().any(|cond| cond.hit(args))
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Read {
    #[default]
    Never,
    Always,
    Unless(Vec<Cond>),
    Sub(Vec<String>),
}

impl Read {
    pub(crate) fn allows(&self, args: &[String]) -> bool {
        match self {
            Read::Never => false,
            Read::Always => true,
            Read::Unless(conds) => !any_hit(conds, args),
            Read::Sub(subs) => args.first().is_some_and(|sub| subs.contains(sub)),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Write {
    #[default]
    None,
    All,
    Last,
    Creates,
    When(Vec<Cond>),
}

impl Write {
    pub(crate) fn destroys(&self, words: &[String]) -> bool {
        match self {
            Write::All | Write::Last => true,
            Write::When(conds) => any_hit(conds, words),
            Write::None | Write::Creates => false,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Cwd {
    #[default]
    Stays,
    Changes,
    Tracked,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Native {
    Cat,
    Head,
    Sed,
    Grep,
    Rg,
    Find,
    Mdfind,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Clip {
    Head,
    Tail,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Gate {
    #[default]
    Destructive,
    Risky,
}

#[derive(Deserialize)]
pub(crate) struct Rule {
    pub(crate) when: Vec<Cond>,
    pub(crate) broad: bool,
    pub(crate) gate: Gate,
    pub(crate) reason: String,
}

#[derive(Deserialize)]
pub(crate) struct SubRule {
    pub(crate) names: BTreeSet<String>,
    pub(crate) when: Vec<Cond>,
    pub(crate) gate: Gate,
    pub(crate) reason: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Hazard {
    #[default]
    None,
    Args(Vec<Rule>),
    Sub {
        skip: BTreeSet<String>,
        rules: Vec<SubRule>,
    },
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Search {
    #[default]
    No,
    Names,
    Contents,
}

#[derive(Deserialize)]
pub(crate) struct Verb {
    names: Vec<String>,
    #[serde(default)]
    pub(crate) read: Read,
    #[serde(default)]
    pub(crate) search: Search,
    #[serde(default)]
    pub(crate) write: Write,
    #[serde(default)]
    pub(crate) cwd: Cwd,
    #[serde(default)]
    pub(crate) native: Option<Native>,
    #[serde(default)]
    pub(crate) clip: Option<Clip>,
    #[serde(default)]
    pub(crate) hazard: Hazard,
}

#[derive(Deserialize)]
pub(crate) struct TextHazard {
    pub(crate) text: String,
    pub(crate) reason: String,
}

#[derive(Deserialize)]
pub(crate) struct Xargs {
    pub(crate) name: String,
    pub(crate) replace: Vec<String>,
    pub(crate) token_flags: Vec<String>,
    pub(crate) default_token: String,
}

#[derive(Deserialize)]
pub(crate) struct Shell {
    pub(crate) shells: BTreeSet<String>,
    pub(crate) interpreters: BTreeSet<String>,
    pub(crate) sourcing: BTreeSet<String>,
    pub(crate) code_flags: Vec<String>,
    pub(crate) escalators: BTreeSet<String>,
    pub(crate) power_off: BTreeSet<String>,
    pub(crate) denied_text: Vec<String>,
    pub(crate) catastrophic: Vec<String>,
    pub(crate) device_write: Vec<String>,
    pub(crate) root_wipes: Vec<String>,
    pub(crate) devices: BTreeSet<String>,
    pub(crate) outside_vars: BTreeSet<String>,
}

#[derive(Deserialize)]
pub(crate) struct Roots {
    pub(crate) top: Vec<String>,
    pub(crate) nested: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct Git {
    pub(crate) modules: String,
    pub(crate) guards: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct Secrets {
    pub(crate) read_files: Vec<String>,
    pub(crate) read_prefixes: Vec<String>,
    pub(crate) read_dirs: Vec<String>,
    pub(crate) write_files: Vec<String>,
    pub(crate) write_prefixes: Vec<String>,
    pub(crate) write_dirs: Vec<String>,
    pub(crate) control: Vec<Vec<String>>,
}

#[derive(Deserialize)]
pub(crate) struct Rules {
    verbs: Vec<Verb>,
    #[serde(skip)]
    index: HashMap<String, usize>,
    pub(crate) inquiry_flags: BTreeSet<String>,
    pub(crate) text_hazards: Vec<TextHazard>,
    pub(crate) wrappers: HashMap<String, BTreeSet<String>>,
    pub(crate) shell: Shell,
    pub(crate) xargs: Xargs,
    pub(crate) roots: Roots,
    pub(crate) git: Git,
    pub(crate) secrets: Secrets,
}

fn load() -> Rules {
    let mut rules: Rules = summonr_core::embedded!("policy rule", "policy/rules.toml");
    rules.index = rules
        .verbs
        .iter()
        .enumerate()
        .flat_map(|(at, verb)| verb.names.iter().map(move |name| (name.clone(), at)))
        .collect();
    rules
}

pub(crate) fn rules() -> &'static Rules {
    static LOADED: OnceLock<Rules> = OnceLock::new();
    LOADED.get_or_init(load)
}

pub(crate) fn verb_row(name: &str) -> Option<&'static Verb> {
    let rules = rules();
    rules.index.get(name).map(|at| &rules.verbs[*at])
}

pub(crate) fn subcommand<'a>(
    args: &'a [String],
    skip: &BTreeSet<String>,
) -> Option<(&'a str, &'a [String])> {
    let mut at = 0;
    loop {
        let word = args.get(at)?;
        if skip.contains(word) {
            at += 2;
        } else if word.starts_with('-') {
            at += 1;
        } else {
            return Some((word, &args[at + 1..]));
        }
    }
}
