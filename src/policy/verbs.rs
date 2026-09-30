mod hazards;
mod table;

pub(crate) use hazards::{INQUIRY_FLAGS, TEXT_HAZARDS};

#[derive(Clone, Copy)]
pub(crate) enum Cond {
    Always,
    Any(&'static [&'static str]),
    All(&'static [&'static str]),
    Prefix(&'static [&'static str]),
    Short(char),
    First(&'static str),
}

impl Cond {
    fn hit(self, args: &[String]) -> bool {
        match self {
            Cond::Always => true,
            Cond::Any(names) => args.iter().any(|arg| names.contains(&arg.as_str())),
            Cond::All(names) => names.iter().all(|name| args.iter().any(|arg| arg == name)),
            Cond::Prefix(names) => args
                .iter()
                .any(|arg| names.iter().any(|name| arg.starts_with(name))),
            Cond::Short(flag) => args
                .iter()
                .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains(flag)),
            Cond::First(name) => args.first().is_some_and(|arg| arg == name),
        }
    }
}

pub(crate) fn any_hit(conds: &[Cond], args: &[String]) -> bool {
    conds.iter().any(|cond| cond.hit(args))
}

#[derive(Clone, Copy)]
pub(crate) enum Read {
    Never,
    Always,
    Unless(&'static [Cond]),
    Sub(&'static [&'static str]),
}

impl Read {
    pub(crate) fn allows(self, args: &[String]) -> bool {
        match self {
            Read::Never => false,
            Read::Always => true,
            Read::Unless(conds) => !any_hit(conds, args),
            Read::Sub(subs) => args.first().is_some_and(|sub| subs.contains(&sub.as_str())),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Write {
    None,
    All,
    Last,
    Creates,
    When(&'static [Cond]),
}

impl Write {
    pub(crate) fn destroys(self, words: &[String]) -> bool {
        match self {
            Write::All | Write::Last => true,
            Write::When(conds) => any_hit(conds, words),
            Write::None | Write::Creates => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cwd {
    Stays,
    Changes,
    Tracked,
}

#[derive(Clone, Copy)]
pub(crate) enum Native {
    Cat,
    Head,
    Sed,
}

#[derive(Clone, Copy)]
pub(crate) enum Clip {
    Head,
    Tail,
}

pub(crate) struct Rule {
    pub(crate) when: &'static [Cond],
    pub(crate) broad: bool,
    pub(crate) reason: &'static str,
}

pub(crate) struct SubRule {
    pub(crate) names: &'static [&'static str],
    pub(crate) when: &'static [Cond],
    pub(crate) reason: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) enum Hazard {
    None,
    Args(&'static [Rule]),
    Sub {
        skip: &'static [&'static str],
        rules: &'static [SubRule],
    },
}

pub(crate) struct Verb {
    pub(crate) name: &'static str,
    pub(crate) read: Read,
    pub(crate) search: bool,
    pub(crate) network: bool,
    pub(crate) write: Write,
    pub(crate) cwd: Cwd,
    pub(crate) native: Option<Native>,
    pub(crate) clip: Option<Clip>,
    pub(crate) hazard: Hazard,
}

pub(crate) fn verb_row(name: &str) -> Option<&'static Verb> {
    table::VERBS.iter().find(|row| row.name == name)
}

pub(crate) fn subcommand<'a>(args: &'a [String], skip: &[&str]) -> Option<(&'a str, &'a [String])> {
    let mut at = 0;
    loop {
        let word = args.get(at)?;
        if skip.contains(&word.as_str()) {
            at += 2;
        } else if word.starts_with('-') {
            at += 1;
        } else {
            return Some((word, &args[at + 1..]));
        }
    }
}
